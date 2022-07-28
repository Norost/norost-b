use crate::arch::amd64::{self, asm::io};
use crate::driver::apic::{io_apic, local_apic};
use core::{fmt, mem::ManuallyDrop};

pub struct Uart {
	port: u16,
}

impl Uart {
	#[allow(dead_code)]
	const DATA: u16 = 0;

	const INTERRUPT_ENABLE: u16 = 1;
	pub const INTERRUPT_DATA_AVAILABLE: u8 = 1 << 0;
	#[allow(dead_code)]
	pub const INTERRUPT_TRANSMITTER_EMPTY: u8 = 1 << 1;
	#[allow(dead_code)]
	pub const INTERRUPT_ERROR: u8 = 1 << 2;
	#[allow(dead_code)]
	pub const INTERRUPT_STATUS_CHANGE: u8 = 1 << 3;

	const LINE_CONTROL: u16 = 3;
	const DLAB_BIT: u8 = 1 << 7;

	pub unsafe fn new(port: u16) -> Self {
		Self { port }
	}

	pub unsafe fn new_no_init(port: u16) -> ManuallyDrop<Self> {
		ManuallyDrop::new(Self { port })
	}

	pub fn send(&mut self, byte: u8) {
		while !self.try_send(byte) {}
	}

	#[must_use = "data may not be written"]
	pub fn try_send(&mut self, byte: u8) -> bool {
		if self.transmit_empty() {
			unsafe { io::out8(self.port, byte) }
			true
		} else {
			false
		}
	}

	#[must_use = "data may be lost if not processed"]
	pub fn try_read(&mut self) -> Option<u8> {
		(!self.receive_empty()).then(|| {
			let b = unsafe { io::in8(self.port) };
			// TODO figure out how to get QEMU to send us the literal newlines instead.
			if b == b'\r' {
				b'\n'
			} else {
				b
			}
		})
	}

	#[must_use = "I/O port space accesses cannot be optimized out"]
	pub fn transmit_empty(&self) -> bool {
		self.line_status() & (1 << 6) != 0
	}

	#[must_use = "I/O port space accesses cannot be optimized out"]
	pub fn receive_empty(&self) -> bool {
		self.line_status() & (1 << 0) == 0
	}

	#[must_use = "I/O port space accesses cannot be optimized out"]
	fn line_status(&self) -> u8 {
		unsafe { io::in8(self.port + 5) }
	}

	fn disable_dlab(&mut self) {
		unsafe {
			let lc = io::in8(self.port + Self::LINE_CONTROL);
			io::out8(self.port + Self::LINE_CONTROL, lc & !Self::DLAB_BIT);
		}
	}

	pub(super) fn enable_interrupts(&mut self, interrupts: u8) {
		unsafe {
			self.disable_dlab();
			let intr = io::in8(self.port + Self::INTERRUPT_ENABLE);
			io::out8(self.port + Self::INTERRUPT_ENABLE, intr | interrupts);
		}
	}

	pub(super) fn disable_interrupts(&mut self, interrupts: u8) {
		unsafe {
			self.disable_dlab();
			let intr = io::in8(self.port + Self::INTERRUPT_ENABLE);
			io::out8(self.port + Self::INTERRUPT_ENABLE, intr & !interrupts);
		}
	}
}

impl fmt::Write for Uart {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		Ok(s.bytes().for_each(|b| self.send(b)))
	}
}

pub(super) unsafe fn init() {
	// We need to do two things on x86:
	// - Set the IDT entry.
	// - Set the I/O APIC to route external interrupts to and IDT
	let com1_irq = 4;
	let com1_vec = amd64::allocate_irq().unwrap();

	unsafe {
		io_apic::set_irq(com1_irq, 0, com1_vec, io_apic::TriggerMode::Level);
		amd64::idt_set(com1_vec.into(), crate::wrap_idt!(irq_handler));
	}
}

extern "C" fn irq_handler() {
	super::table::irq_handler();
	local_apic::get().eoi.set(0);
}
