#![no_std]
#![feature(start)]
#![feature(const_trait_impl, inline_const)]
#![feature(let_else)]
#![deny(unused_must_use)]

extern crate alloc;

macro_rules! log {
	($($arg:tt)+) => {{
		let _ = rt::io::stderr().map(|o| core::writeln!(o, $($arg)+));
	}};
}

mod keyboard;

//use acpi::{fadt::Fadt, sdt::Signature, AcpiHandler, AcpiTables};
use alloc::boxed::Box;
use async_std::{
	io::{Read, Write},
	object::RefAsyncObject,
	task,
};
use driver_utils::os::{
	portio::PortIo,
	stream_table::{JobId, Request, Response, StreamTable},
};
use futures_util::future;
use rt as _;
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	task::block_on(main())
}

async fn main() -> ! {
	let (mut ps2, [dev1, dev2]) = Ps2::init();
	let mut buf = [0; 16];

	let dev1 = dev1.unwrap();

	let tbl = {
		let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 256 }).unwrap();
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

	let tbl_loop = async {
		let mut buf = [0; 16];
		loop {
			tbl_notify.read(()).await.0.unwrap();
			let mut flush = false;
			const KEYBOARD_STREAM_HANDLE: rt::Handle = rt::Handle::MAX - 1;
			while let Some((handle, req)) = tbl.dequeue() {
				let (job_id, resp) = match req {
					Request::Open { job_id, path } => (job_id, {
						let l = path.len();
						path.copy_to(0, &mut buf[..l]);
						path.manual_drop();
						if &buf[..l] == b"keyboard/stream" {
							Response::Handle(KEYBOARD_STREAM_HANDLE)
						} else {
							Response::Error(rt::Error::DoesNotExist)
						}
					}),
					Request::Read { job_id, amount } => (
						job_id,
						match handle {
							rt::Handle::MAX => Response::Error(rt::Error::InvalidOperation),
							KEYBOARD_STREAM_HANDLE => {
								if amount < 4 {
									Response::Error(rt::Error::InvalidData)
								} else if let Some((job_id, d)) = dev1.add_reader(job_id, &mut buf)
								{
									let mut data = tbl.alloc(d.len()).expect("out of buffers");
									data.copy_from(0, d);
									Response::Data(data)
								} else {
									continue;
								}
							}
							_ => unreachable!(),
						},
					),
					Request::Close => continue,
					Request::Create { job_id, .. }
					| Request::Destroy { job_id, .. }
					| Request::Seek { job_id, .. }
					| Request::Share { job_id, .. }
					| Request::Write { job_id, .. }
					| Request::GetMeta { job_id, .. }
					| Request::SetMeta { job_id, .. } => (job_id, Response::Error(rt::Error::InvalidOperation)),
				};
				tbl.enqueue(job_id, resp);
				flush = true;
			}
			flush.then(|| tbl.flush());
		}
	};
	let dev1_loop = async {
		loop {
			dev1_intr.read(()).await.0.unwrap();
			if let Some((job_id, d)) = dev1.handle_interrupt(&mut ps2, &mut buf) {
				let mut data = tbl.alloc(d.len()).expect("out of buffers");
				data.copy_from(0, d);
				tbl.enqueue(job_id, Response::Data(data));
				tbl.flush();
			}
			dev1_intr.write(()).await.0.unwrap();
		}
	};
	futures_util::pin_mut!(tbl_loop);
	futures_util::pin_mut!(dev1_loop);
	future::select(tbl_loop, dev1_loop).await.factor_first().0
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

const PORT_ACKNOWLEDGE: u8 = 0xfa;
const PORT_RESEND: u8 = 0xfe;

const DEVICE_MF2_KEYBOARD: u8 = 0xab;

// TODO determine what a reasonable timeout is.
const TIMEOUT: u32 = 100_000;

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
		for _ in 0..TIMEOUT {
			if self.io.in8(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(self.io.out8(COMMAND, cmd as u8));
			}
		}
		Err(Timeout)
	}

	fn read_data_nowait(&self) -> Result<u8, Timeout> {
		(self.io.in8(STATUS) & STATUS_OUTPUT_FULL != 0)
			.then(|| self.io.in8(DATA))
			.ok_or(Timeout)
	}

	fn read_data(&self) -> Result<u8, Timeout> {
		for _ in 0..TIMEOUT {
			if self.io.in8(STATUS) & STATUS_OUTPUT_FULL != 0 {
				return Ok(self.io.in8(DATA));
			}
		}
		Err(Timeout)
	}

	fn write_data(&self, byte: u8) -> Result<(), Timeout> {
		for _ in 0..TIMEOUT {
			if self.io.in8(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(self.io.out8(DATA, byte));
			}
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
		let (irq, i) = match port {
			Port::P1 => (1, 0),
			Port::P2 => (4, 1),
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

	pub fn init() -> (Self, [Option<Box<dyn Device>>; 2]) {
		// https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller

		let mut slf = Self {
			io: PortIo::new().unwrap(),
		};
		let mut devices: [Option<Box<dyn Device>>; 2] = [None, None];

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
			let two_channels = slf.read_data().unwrap() & CTRL_CFG_PORT_2_CLOCK_DISABLED == 0;
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
			for (i, port) in [Port::P1]
				.into_iter()
				.chain(two_channels.then(|| Port::P2))
				.enumerate()
			{
				slf.write_command(Command::EnablePort1).unwrap();
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
							devices[i] = Some(Box::new(keyboard::Keyboard::init(&mut slf, port)))
						}
						&[a] => log!("{:?}: unsupported device {:#02x}", port, a),
						&[a, b] => log!("{:?}: unsupported device {:#02x}{:02x}", port, a, b),
						_ => unreachable!(),
				}
			}

			(slf, devices)
		}
	}
}
