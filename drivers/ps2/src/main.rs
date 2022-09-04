#![no_std]
#![feature(start)]
#![feature(const_trait_impl, inline_const)]
#![feature(let_else)]
#![deny(unused_must_use)]

extern crate alloc;

macro_rules! log {
	($($arg:tt)+) => {
		rt::eprintln!($($arg)+)
	};
}

mod keyboard;
mod lossy_ring_buffer;
mod mouse;

//use acpi::{fadt::Fadt, sdt::Signature, AcpiHandler, AcpiTables};
use {
	alloc::boxed::Box,
	async_std::{
		io::{Read, Write},
		object::RefAsyncObject,
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
	let (ps2, [dev1, dev2]) = Ps2::init();
	let mut buf = [0; 16];

	let dev1 = dev1.unwrap();
	let dev2 = dev2.unwrap();

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

	let tbl_notify = RefAsyncObject::from(tbl.notifier());
	let dev1_intr = RefAsyncObject::from(dev1.interrupter());
	let dev2_intr = RefAsyncObject::from(dev2.interrupter());

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
		dev_intr: RefAsyncObject<'_>,
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
	let dev1_loop = f_loop(&tbl, &ps2, &*dev1, dev1_intr);
	let dev2_loop = f_loop(&tbl, &ps2, &*dev2, dev2_intr);
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

const DEVICE_MF2_KEYBOARD: u8 = 0xab;

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
	#[allow(dead_code)]
	WriteNextByteToPort2Input = 0xd4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Port {
	P1,
	P2,
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

	fn interrupter(&self) -> rt::RefObject<'_>;
}

pub struct Ps2 {
	io: PortIo,
}

impl Ps2 {
	fn write_command(&self, cmd: Command) -> Result<(), Timeout> {
		for _ in 0..TIMEOUT_MS {
			if self.io.in8(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(self.io.out8(COMMAND, cmd as u8));
			}
			rt::thread::sleep(Duration::from_millis(1));
		}
		Err(Timeout)
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

	fn write_data(&self, byte: u8) -> Result<(), Timeout> {
		for _ in 0..TIMEOUT_MS {
			if self.io.in8(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(self.io.out8(DATA, byte));
			}
			rt::thread::sleep(Duration::from_millis(1));
		}
		Err(Timeout)
	}

	fn write_raw_port_command(&self, port: Port, cmd: u8) -> Result<(), Timeout> {
		match port {
			Port::P1 => {}
			Port::P2 => self.write_command(Command::WriteNextByteToPort2Input)?,
		}
		self.write_data(cmd)
	}

	fn write_port_command(&self, port: Port, cmd: PortCommand) -> Result<(), Timeout> {
		self.write_raw_port_command(port, cmd as u8)
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

	fn read_port_data_with_acknowledge(&self) -> Result<(), ReadAckError> {
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

		// Enable interrupt
		self.write_command(Command::ReadControllerConfiguration)
			.unwrap();
		let cfg = self.read_data().unwrap()
			| match port {
				Port::P1 => CTRL_CFG_PORT_1_INTERRUPT_ENABLED,
				Port::P2 => CTRL_CFG_PORT_2_INTERRUPT_ENABLED,
			};
		self.write_command(Command::WriteControllerConfiguration)
			.unwrap();
		self.write_data(cfg).unwrap();

		intr
	}

	fn init() -> (Self, [Option<Box<dyn Device>>; 2]) {
		// https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller
		let mut slf = Self { io: PortIo::new().unwrap() };
		let mut devices: [Option<Box<dyn Device>>; 2] = [None, None];
		// Because shit's just broken on my laptop. Hoo fucking ha
		let mut two_channels = rt::args::args().find(|a| a == b"disable-port2").is_none();

		{
			// Disable devices
			slf.write_command(Command::DisablePort1).unwrap();
			slf.write_command(Command::DisablePort2).unwrap();

			// Ensure the output buffer is flushed
			let _ = slf.io.in8(DATA);

			// Setup the controller configuration byte up properly
			slf.write_command(Command::ReadControllerConfiguration)
				.unwrap();
			let cfg = slf.read_data().unwrap()
				& !(CTRL_CFG_PORT_1_INTERRUPT_ENABLED
					| CTRL_CFG_PORT_2_INTERRUPT_ENABLED
					| CTRL_CFG_PORT_1_TRANSLATION);
			slf.write_command(Command::WriteControllerConfiguration)
				.unwrap();
			slf.write_data(cfg).unwrap();

			// Perform self test
			slf.write_command(Command::Test).unwrap();
			match slf.read_data().unwrap() {
				TEST_PASSED => {
					// Write cfg again as the controller may have been reset
					slf.write_command(Command::WriteControllerConfiguration)
						.unwrap();
					slf.write_data(cfg).unwrap();
				}
				TEST_FAILED => panic!("8042 controller test failed"),
				data => panic!("invalid test status from 8042 controller: {:#x}", data),
			}

			// Test if it's a 2 channel controller
			slf.write_command(Command::EnablePort2).unwrap();
			slf.write_command(Command::ReadControllerConfiguration)
				.unwrap();
			two_channels &= slf.read_data().unwrap() & CTRL_CFG_PORT_2_CLOCK_DISABLED == 0;
			slf.write_command(Command::DisablePort2).unwrap();

			// Test ports
			let test = |cmd, i| {
				slf.write_command(cmd).unwrap();
				match slf.read_data().unwrap() {
					PORT_TEST_PASSED => {}
					PORT_TEST_CLOCK_STUCK_LOW => {
						panic!("8042 controller port {} clock stuck low", i)
					}
					PORT_TEST_CLOCK_STUCK_HIGH => {
						panic!("8042 controller port {} clock stuck high", i)
					}
					PORT_TEST_DATA_STUCK_LOW => {
						panic!("8042 controller port {} data stuck low", i)
					}
					PORT_TEST_DATA_STUCK_HIGH => {
						panic!("8042 controller port {} data stuck high", i)
					}
					data => panic!(
						"8042 controller invalid port {} test result: {:#x}",
						i, data
					),
				}
			};
			test(Command::TestPort1, 1);
			if two_channels {
				test(Command::TestPort2, 2);
			}

			// Initialize drivers for any detected PS/2 devices
			for (i, (port, enable_cmd, disable_cmd)) in
				[(Port::P1, Command::EnablePort1, Command::DisablePort2)]
					.into_iter()
					.chain(
						two_channels
							.then(|| (Port::P2, Command::EnablePort2, Command::DisablePort2)),
					)
					.enumerate()
			{
				slf.write_command(enable_cmd).unwrap();

				// Reset to clear buffer
				slf.write_port_command(port, PortCommand::Reset).unwrap();
				slf.read_port_data_with_acknowledge().unwrap();
				if slf
					.read_port_data()
					.map_or(true, |c| c != PORT_SELF_TEST_PASSED)
				{
					slf.write_command(disable_cmd).unwrap();
					log!("{:?}: reset & self test failed", port);
					continue;
				}

				// QEMU sends mouse_type because reasons.
				let _ = slf.read_port_data();

				slf.write_port_command(port, PortCommand::DisableScanning)
					.unwrap();
				slf.read_port_data_with_acknowledge().unwrap();
				slf.write_port_command(port, PortCommand::Identify).unwrap();
				slf.read_port_data_with_acknowledge().unwrap();
				let mut id = [0; 2];
				let id = match slf.read_port_data() {
					Ok(a) => match slf.read_port_data() {
						Ok(b) => {
							id = [a, b];
							&id[..2]
						}
						Err(Timeout) => {
							id = [a, 0];
							&id[..1]
						}
					},
					Err(Timeout) => &id[..0],
				};
				match id {
						// Ancient AT keyboard with translation
						&[]
						// MF2 keyboard with translation
						| &[DEVICE_MF2_KEYBOARD, 0x41]
						| &[DEVICE_MF2_KEYBOARD, 0xc1]
						// MF2 keyboard without translation
						| &[DEVICE_MF2_KEYBOARD, 0x83]
						=> {
							log!("{:?}: found keyboard", port);
							devices[i] = Some(Box::new(keyboard::Keyboard::init(&mut slf, port)));
							continue;
						}
						&[0x00] => {
							log!("{:?}: found mouse", port);
							devices[i] = Some(Box::new(mouse::Mouse::init(&mut slf, port)));
							continue;
						}
						&[a] => log!("{:?}: unsupported device {:#02x}", port, a),
						&[a, b] => log!("{:?}: unsupported device {:#02x}{:02x}", port, a, b),
						_ => unreachable!(),
				}
				slf.write_command(disable_cmd).unwrap();
			}

			(slf, devices)
		}
	}
}
