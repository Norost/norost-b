#![no_std]
#![feature(start)]
#![feature(const_trait_impl, inline_const)]
#![feature(let_else)]
#![deny(unused_must_use)]

extern crate alloc;

macro_rules! log {
	($fmt:literal) => {{
		rt::eprintln!(concat!("[PS2] ", $fmt));
	}};
	($($arg:tt)+) => {{
		rt::eprint!("[PS2] ");
		rt::eprintln!($($arg)+);
	}};
}

mod keyboard;
mod lossy_ring_buffer;
mod mouse;

//use acpi::{fadt::Fadt, sdt::Signature, AcpiHandler, AcpiTables};
use {
	alloc::boxed::Box,
	async_std::{
		io::{Read, Write},
		object::{AsyncObject, RefAsyncObject},
		task,
	},
	core::{cell::RefCell, time::Duration},
	driver_utils::os::{
		portio::PortIo,
		stream_table::{JobId, Request, Response, StreamTable},
	},
	futures_util::future,
	lossy_ring_buffer::LossyRingBuffer,
	rt::{self as _, Error, Handle, NewObject, Object},
	rt_default as _,
};

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	task::block_on(main())
}

async fn main() -> ! {
	let (mut ps2, dev1, dev2) = Ps2::init();
	let mut buf = [0; 16];

	let tbl = {
		let (buf, _) = Object::new(NewObject::SharedMemory { size: 256 }).unwrap();
		StreamTable::new(&buf, 8.try_into().unwrap(), 16 - 1)
	};
	rt::io::file_root()
		.unwrap()
		.create(b"ps2")
		.unwrap()
		.share(tbl.public())
		.unwrap();

	// Install IRQs
	let dev1_intr = ps2.install_interrupt(Port::P1).into();
	let dev2_intr = ps2.install_interrupt(Port::P2).into();

	let tbl_notify = RefAsyncObject::from(tbl.notifier());

	let tbl_loop = async {
		let mut buf = [0; 16];
		loop {
			tbl_notify.read(()).await.0.unwrap();
			let mut flush = false;
			const KEYBOARD_HANDLE: Handle = Handle::MAX - 1;
			const MOUSE_HANDLE: Handle = Handle::MAX - 2;
			while let Some((handle, mut job_id, req)) = tbl.dequeue() {
				let resp = match req {
					Request::Open { path } => match &*path.copy_into(&mut buf).0 {
						b"keyboard" => Response::Handle(KEYBOARD_HANDLE),
						b"mouse" => Response::Handle(MOUSE_HANDLE),
						_ => Response::Error(rt::Error::DoesNotExist),
					},
					Request::Read { .. } if handle == Handle::MAX => {
						Response::Error(Error::InvalidOperation)
					}
					Request::Read { amount } if handle == KEYBOARD_HANDLE => {
						if amount < 4 {
							Response::Error(Error::InvalidData)
						} else if let Some((id, d)) = dev1.add_reader(job_id, &mut buf) {
							job_id = id;
							let data = tbl.alloc(d.len()).expect("out of buffers");
							data.copy_from(0, d);
							Response::Data(data)
						} else {
							continue;
						}
					}
					Request::Read { amount } if handle == MOUSE_HANDLE => {
						if amount < 4 {
							Response::Error(Error::InvalidData)
						} else if let Some((id, d)) = dev2.add_reader(job_id, &mut buf) {
							job_id = id;
							let data = tbl.alloc(d.len()).expect("out of buffers");
							data.copy_from(0, d);
							Response::Data(data)
						} else {
							continue;
						}
					}
					Request::Close => continue,
					_ => Response::Error(rt::Error::InvalidOperation),
				};
				tbl.enqueue(job_id, resp);
				flush = true;
			}
			flush.then(|| tbl.flush());
		}
	};
	let ps2 = RefCell::new(ps2);
	async fn f_loop(
		tbl: &StreamTable,
		ps2: &RefCell<Ps2>,
		dev: &dyn Device,
		dev_intr: AsyncObject,
	) -> ! {
		let mut buf = [0; 16];
		loop {
			dev_intr.read(()).await.0.unwrap();
			if let Some((job_id, d)) = dev.handle_interrupt(&mut ps2.borrow_mut(), &mut buf) {
				let data = tbl.alloc(d.len()).expect("out of buffers");
				data.copy_from(0, d);
				tbl.enqueue(job_id, Response::Data(data));
				tbl.flush();
			}
			dev_intr.write(()).await.0.unwrap();
		}
	};
	let dev1_loop = f_loop(&tbl, &ps2, &dev1, dev1_intr);
	let dev2_loop = f_loop(&tbl, &ps2, &dev2, dev2_intr);
	futures_util::pin_mut!(tbl_loop);
	futures_util::pin_mut!(dev1_loop);
	futures_util::pin_mut!(dev2_loop);
	match future::select(tbl_loop, future::select(dev1_loop, dev2_loop)).await {
		future::Either::Left(v) => v.0,
		future::Either::Right(v) => match v.0 {
			future::Either::Left(v) => v.0,
			future::Either::Right(v) => v.0,
		},
	}
}

const DATA: u16 = 0x60;
const COMMAND: u16 = 0x64;
const STATUS: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;

const CTRL_CFG_PORT_1_INTERRUPT_ENABLED: u8 = 1 << 0;
const CTRL_CFG_PORT_2_INTERRUPT_ENABLED: u8 = 1 << 1;
#[allow(dead_code)]
const CTRL_CFG_SYSTEM_FLAG: u8 = 1 << 2;
#[allow(dead_code)]
const CTRL_CFG_PORT_1_CLOCK_DISABLED: u8 = 1 << 4;
const CTRL_CFG_PORT_2_CLOCK_DISABLED: u8 = 1 << 5;
const CTRL_CFG_PORT_1_TRANSLATION: u8 = 1 << 6;

const TEST_PASSED: u8 = 0x55;
const TEST_FAILED: u8 = 0xfc;

const PORT_TEST_PASSED: u8 = 0x00;
const PORT_TEST_CLOCK_STUCK_LOW: u8 = 0x01;
const PORT_TEST_CLOCK_STUCK_HIGH: u8 = 0x02;
const PORT_TEST_DATA_STUCK_LOW: u8 = 0x03;
const PORT_TEST_DATA_STUCK_HIGH: u8 = 0x04;

const PORT_SELF_TEST_PASSED: u8 = 0xaa;
const PORT_ACKNOWLEDGE: u8 = 0xfa;
const PORT_RESEND: u8 = 0xfe;

// TODO determine what a reasonable timeout is.
const TIMEOUT_MS: u32 = 100;

enum Command {
	ReadControllerConfiguration = 0x20,
	WriteControllerConfiguration = 0x60,
	DisablePort2 = 0xa7,
	#[allow(dead_code)]
	EnablePort2 = 0xa8,
	#[allow(dead_code)]
	TestPort2 = 0xa9,
	Test = 0xaa,
	#[allow(dead_code)]
	TestPort1 = 0xab,
	#[allow(dead_code)]
	DiagonosticDump = 0xac,
	DisablePort1 = 0xad,
	#[allow(dead_code)]
	EnablePort1 = 0xae,
	#[allow(dead_code)]
	ReadControllerInput = 0xc0,
	/// Copy 3:0 to 7:4
	#[allow(dead_code)]
	CopyLowerBitsToHigherStatus = 0xc1,
	#[allow(dead_code)]
	CopyHigherrBitsToHigherStatus = 0xc2,
	#[allow(dead_code)]
	ReadControllerOutput = 0xd0,
	#[allow(dead_code)]
	WriteNextByteToControllerOutput = 0xd1,
	#[allow(dead_code)]
	WriteNextByteToPort1Output = 0xd2,
	#[allow(dead_code)]
	WriteNextByteToPort2Output = 0xd3,
	WriteNextByteToPort2Input = 0xd4,
}

enum PortCommand {
	Identify = 0xf2,
	EnableScanning = 0xf4,
	DisableScanning = 0xf5,
	Reset = 0xff,
}

#[derive(Debug, PartialEq)]
struct Timeout;

#[derive(Debug, PartialEq)]
enum ReadError {
	Timeout,
	Resend,
}

#[derive(Debug, PartialEq)]
enum ReadAckError {
	Timeout,
	Resend,
	UnexpectedResponse(u8),
}

trait Device {
	#[must_use]
	fn add_reader<'a>(&self, job: JobId, buf: &'a mut [u8; 16]) -> Option<(JobId, &'a [u8])>;

	#[must_use]
	fn handle_interrupt<'a>(
		&self,
		ps2: &mut Ps2,
		buf: &'a mut [u8; 16],
	) -> Option<(JobId, &'a [u8])>;
}

pub struct Ps2 {
	io: PortIo,
}

pub enum Port {
	P1,
	P2,
}

// Based on https://github.com/klange/toaruos/blob/bb1c30d/kernel/arch/x86_64/ps2hid.c#L318
// which hopefully works for everything
impl Ps2 {
	fn wait_input(&self) -> Result<(), Timeout> {
		for _ in 0..TIMEOUT_MS {
			if self.io.in8(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(());
			}
			rt::thread::sleep(Duration::from_millis(1));
		}
		Err(Timeout)
	}

	fn wait_output(&self) -> Result<(), Timeout> {
		for _ in 0..TIMEOUT_MS {
			if self.io.in8(STATUS) & STATUS_OUTPUT_FULL != 0 {
				return Ok(());
			}
			rt::thread::sleep(Duration::from_millis(1));
		}
		Err(Timeout)
	}

	fn write_cmd(&self, cmd: Command) -> Result<(), Timeout> {
		self.wait_input()?;
		self.io.out8(COMMAND, cmd as u8);
		Ok(())
	}

	fn write_data(&self, arg: u8) -> Result<(), Timeout> {
		self.wait_input()?;
		self.io.out8(DATA, arg);
		Ok(())
	}

	fn read_data_nowait(&self) -> Result<u8, Timeout> {
		(self.io.in8(STATUS) & STATUS_OUTPUT_FULL != 0)
			.then(|| self.io.in8(DATA))
			.ok_or(Timeout)
	}

	fn read_data(&self) -> Result<u8, Timeout> {
		for _ in 0..TIMEOUT_MS {
			if self.io.in8(STATUS) & STATUS_OUTPUT_FULL != 0 {
				return Ok(self.io.in8(DATA));
			}
			rt::thread::sleep(Duration::from_millis(1));
		}
		Err(Timeout)
	}

	fn read_port_data(&self) -> Result<u8, Timeout> {
		self.read_data()
	}

	fn read_port_data_nowait(&self) -> Result<u8, Timeout> {
		self.read_data_nowait()
	}

	fn read_port_data_with_resend(&self) -> Result<u8, ReadError> {
		match self.read_port_data() {
			Ok(PORT_RESEND) => Err(ReadError::Resend),
			Ok(data) => Ok(data),
			Err(Timeout) => Err(ReadError::Timeout),
		}
	}

	fn read_port_acknowledge(&self) -> Result<(), ReadAckError> {
		match self.read_port_data() {
			Ok(PORT_ACKNOWLEDGE) => Ok(()),
			Ok(PORT_RESEND) => Err(ReadAckError::Resend),
			Ok(data) => Err(ReadAckError::UnexpectedResponse(data)),
			Err(Timeout) => Err(ReadAckError::Timeout),
		}
	}

	fn install_interrupt(&mut self, port: Port) -> rt::Object {
		// Configure interrupt
		use driver_utils::os::interrupt;
		let irq = match port {
			Port::P1 => 1,
			Port::P2 => 12,
		};
		let intr = interrupt::allocate(Some(irq), interrupt::TriggerMode::Level);

		intr
	}

	fn write_keyboard(&mut self, b: u8) {
		self.write_data(b).unwrap();
		self.read_port_acknowledge().unwrap();
	}

	fn write_mouse(&mut self, b: u8) {
		self.write_cmd(Command::WriteNextByteToPort2Input).unwrap();
		self.write_data(b).unwrap();
		//self.read_port_acknowledge().unwrap();
		let _ = self.read_port_acknowledge();
	}

	fn init() -> (Self, keyboard::Keyboard, mouse::Mouse) {
		// https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
		let mut slf = Self { io: PortIo::new().unwrap() };

		log!("disable ports");
		slf.write_cmd(Command::DisablePort1).unwrap();
		slf.write_cmd(Command::DisablePort2).unwrap();

		log!("clearing input buffer");
		while slf.read_data_nowait().is_ok() {}

		log!("enable interrupts & disable translation");
		slf.write_cmd(Command::ReadControllerConfiguration).unwrap();
		let cfg = slf.read_data().unwrap()
			| CTRL_CFG_PORT_1_INTERRUPT_ENABLED
			| CTRL_CFG_PORT_2_INTERRUPT_ENABLED;
		let cfg = cfg & !CTRL_CFG_PORT_1_TRANSLATION;
		slf.write_cmd(Command::WriteControllerConfiguration)
			.unwrap();
		slf.write_data(cfg).unwrap();

		log!("enable ports");
		slf.write_cmd(Command::EnablePort1).unwrap();
		slf.write_cmd(Command::EnablePort2).unwrap();

		log!("set keyboard scancode set 2");
		slf.write_keyboard(keyboard::cmd::GET_SET_SCANCODE_SET);
		slf.write_keyboard(2);

		log!("set mouse defaults & enable");
		slf.write_mouse(mouse::cmd::SET_DEFAULTS);
		slf.write_mouse(mouse::cmd::DATA_ON);

		log!("load keyboard driver");
		let keyboard = keyboard::Keyboard::new();
		log!("load mouse driver");
		let mouse = mouse::Mouse::new();

		(slf, keyboard, mouse)
	}
}
