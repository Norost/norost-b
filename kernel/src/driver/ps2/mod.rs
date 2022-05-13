mod keyboard;

use crate::{arch::amd64::asm::io, object_table::Root};
use acpi::{fadt::Fadt, sdt::Signature, AcpiHandler, AcpiTable, AcpiTables};
use core::ptr;

const DATA: u16 = 0x60;
const COMMAND: u16 = 0x64;
const STATUS: u16 = 0x64;

const STATUS_OUTPUT_FULL: u8 = 1 << 0;
const STATUS_INPUT_FULL: u8 = 1 << 1;

const CTRL_CFG_PORT_1_INTERRUPT_ENABLED: u8 = 1 << 0;
const CTRL_CFG_PORT_2_INTERRUPT_ENABLED: u8 = 1 << 1;
#[allow(dead_code)]
const CTRL_CFG_SYSTEM_FLAG: u8 = 1 << 2;
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

#[derive(Clone, Copy)]
enum Port {
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

// TODO these commands may actually be safe. Whether they're safe depends on how they can influence
// the rest of the system (e.g. can any of these commands corrupt system memory?)

unsafe fn write_command(cmd: Command) -> Result<(), Timeout> {
	unsafe {
		for _ in 0..TIMEOUT {
			if io::inb(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(io::outb(COMMAND, cmd as u8));
			}
		}
		Err(Timeout)
	}
}

#[must_use = "read_data has side effects"]
unsafe fn read_data_nowait() -> Result<u8, Timeout> {
	unsafe {
		(io::inb(STATUS) & STATUS_OUTPUT_FULL != 0)
			.then(|| io::inb(DATA))
			.ok_or(Timeout)
	}
}

#[must_use = "read_data has side effects"]
unsafe fn read_data() -> Result<u8, Timeout> {
	unsafe {
		for _ in 0..TIMEOUT {
			if io::inb(STATUS) & STATUS_OUTPUT_FULL != 0 {
				return Ok(io::inb(DATA));
			}
		}
		Err(Timeout)
	}
}

unsafe fn write_data(byte: u8) -> Result<(), Timeout> {
	unsafe {
		for _ in 0..TIMEOUT {
			if io::inb(STATUS) & STATUS_INPUT_FULL == 0 {
				return Ok(io::outb(DATA, byte));
			}
		}
		Err(Timeout)
	}
}

unsafe fn write_raw_port_command(port: Port, cmd: u8) -> Result<(), Timeout> {
	unsafe {
		match port {
			Port::P1 => {}
			Port::P2 => write_command(Command::WriteNextByteToPort2Input)?,
		}
		write_data(cmd)
	}
}

unsafe fn write_port_command(port: Port, cmd: PortCommand) -> Result<(), Timeout> {
	unsafe { write_raw_port_command(port, cmd as u8) }
}

/// # Note
///
/// This does not disambiguate between ports
#[must_use = "read_port_data has side effects"]
unsafe fn read_port_data() -> Result<u8, Timeout> {
	unsafe { read_data() }
}

/// # Note
///
/// This does not disambiguate between ports
#[must_use = "read_port_data_nowait has side effects"]
unsafe fn read_port_data_nowait() -> Result<u8, Timeout> {
	unsafe { read_data_nowait() }
}

#[derive(Debug, PartialEq)]
enum ReadError {
	Timeout,
	Resend,
}

/// # Note
///
/// This does not disambiguate between ports
#[must_use = "read_port_data_with_resend has side effects"]
unsafe fn read_port_data_with_resend() -> Result<u8, ReadError> {
	match unsafe { read_port_data() } {
		Ok(PORT_RESEND) => Err(ReadError::Resend),
		Ok(data) => Ok(data),
		Err(Timeout) => Err(ReadError::Timeout),
	}
}

#[derive(Debug, PartialEq)]
enum ReadAckError {
	Timeout,
	Resend,
	UnexpectedResponse(u8),
}

/// # Note
///
/// This does not disambiguate between ports
#[must_use = "read_port_data_with_acknowledge has side effects"]
unsafe fn read_port_data_with_acknowledge() -> Result<(), ReadAckError> {
	match unsafe { read_port_data() } {
		Ok(PORT_ACKNOWLEDGE) => Ok(()),
		Ok(PORT_RESEND) => Err(ReadAckError::Resend),
		Ok(data) => Err(ReadAckError::UnexpectedResponse(data)),
		Err(Timeout) => Err(ReadAckError::Timeout),
	}
}

fn install_irq(port: Port, handler: crate::arch::amd64::IDTEntry) {
	let irq = match port {
		Port::P1 => 1,
		Port::P2 => 4,
	};
	unsafe {
		// Configure interrupt
		use crate::{arch::amd64, driver::apic::io_apic};
		let idt = amd64::allocate_irq().unwrap();
		amd64::idt_set(idt.into(), handler);
		io_apic::set_irq(irq, 0, idt, io_apic::TriggerMode::Level);

		// Enable interrupt
		write_command(Command::ReadControllerConfiguration).unwrap();
		let cfg = read_data().unwrap()
			| match port {
				Port::P1 => CTRL_CFG_PORT_1_INTERRUPT_ENABLED,
				Port::P2 => CTRL_CFG_PORT_2_INTERRUPT_ENABLED,
			};
		write_command(Command::WriteControllerConfiguration).unwrap();
		write_data(cfg).unwrap();
	}
}

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init_acpi(tables: &AcpiTables<impl AcpiHandler>, root: &Root) {
	// https://wiki.osdev.org/%228042%22_PS/2_Controller#Initialising_the_PS.2F2_Controller

	// Ensure the PS/2 controller exists
	let fadt = unsafe {
		tables
			.get_sdt::<Fadt>(Signature::FADT)
			.expect("error parsing ACPI tables")
			.expect("no FADT table")
	};

	if tables.revision < 2 {
		// There is no iapc_boot_arch in this version of the FADT.
		// Just assume the controller is present.
	} else {
		let iapc_boot_arch = unsafe { ptr::addr_of!(fadt.iapc_boot_arch).read_unaligned() };
		if !iapc_boot_arch.motherboard_implements_8042() {
			warn!("no 8042 controller is present");
			return;
		}
	}

	let two_channels;

	unsafe {
		// Disable devices
		write_command(Command::DisablePort1).unwrap();
		write_command(Command::DisablePort2).unwrap();

		// Ensure the output buffer is flushed
		let _ = io::inb(DATA);

		// Setup the controller configuration byte up properly
		write_command(Command::ReadControllerConfiguration).unwrap();
		let cfg = read_data().unwrap()
			& !(CTRL_CFG_PORT_1_INTERRUPT_ENABLED
				| CTRL_CFG_PORT_2_INTERRUPT_ENABLED
				| CTRL_CFG_PORT_1_TRANSLATION);
		write_command(Command::WriteControllerConfiguration).unwrap();
		write_data(cfg).unwrap();

		// Perform self test
		write_command(Command::Test).unwrap();
		match read_data().unwrap() {
			TEST_PASSED => {
				// Write cfg again as the controller may have been reset
				write_command(Command::WriteControllerConfiguration).unwrap();
				write_data(cfg).unwrap();
			}
			TEST_FAILED => panic!("8042 controller test failed"),
			data => panic!("invalid test status from 8042 controller: {:#x}", data),
		}

		// Test if it's a 2 channel controller
		write_command(Command::EnablePort2).unwrap();
		write_command(Command::ReadControllerConfiguration).unwrap();
		two_channels = read_data().unwrap() & CTRL_CFG_PORT_2_CLOCK_DISABLED == 0;
		write_command(Command::DisablePort2).unwrap();

		dbg!(two_channels);

		// Test ports
		let test = |cmd, i| {
			write_command(cmd).unwrap();
			match read_data().unwrap() {
				PORT_TEST_PASSED => {}
				PORT_TEST_CLOCK_STUCK_LOW => panic!("8042 controller port {} clock stuck low", i),
				PORT_TEST_CLOCK_STUCK_HIGH => panic!("8042 controller port {} clock stuck high", i),
				PORT_TEST_DATA_STUCK_LOW => panic!("8042 controller port {} data stuck low", i),
				PORT_TEST_DATA_STUCK_HIGH => panic!("8042 controller port {} data stuck high", i),
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
		let port = Port::P1;
		write_command(Command::EnablePort1).unwrap();
		write_port_command(port, PortCommand::DisableScanning).unwrap();
		read_port_data_with_acknowledge().unwrap();
		write_port_command(port, PortCommand::Identify).unwrap();
		read_port_data_with_acknowledge().unwrap();
		let mut id = [0; 2];
		let id = match read_port_data() {
			Ok(a) => match read_port_data() {
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
				#[cfg(feature = "driver-ps2-keyboard")]
				keyboard::init(port, root);
				#[cfg(not(feature = "driver-ps2-keyboard"))]
				info!("ps2: no driver for keyboard (device type {:#02x})", d);
			}
			&[a] => info!("ps2: unsupported device {:#02x}", a),
			&[a, b] => info!("ps2: unsupported device {:#02x}{:02x}", a, b),
			_ => unreachable!(),
		}
	}
}
