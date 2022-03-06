use crate::arch::amd64::asm::io;
use core::fmt;

pub struct Uart {
	port: u16,
}

impl Uart {
	pub unsafe fn new(port: u16) -> Self {
		Self { port }
	}

	pub fn send(&mut self, byte: u8) {
		while !self.try_send(byte) {}
	}

	#[must_use = "data may not be written"]
	pub fn try_send(&mut self, byte: u8) -> bool {
		if self.transmit_empty() {
			unsafe { io::outb(self.port, byte) }
			true
		} else {
			false
		}
	}

	#[must_use = "data may be lost if not processed"]
	pub fn read(&mut self) -> u8 {
		loop {
			if let Some(b) = self.try_read() {
				return b;
			}
		}
	}

	#[must_use = "data may be lost if not processed"]
	pub fn try_read(&mut self) -> Option<u8> {
		(!self.receive_empty()).then(|| {
			let b = unsafe { io::inb(self.port) };
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
		unsafe { io::inb(self.port + 5) }
	}
}

impl fmt::Write for Uart {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		Ok(s.bytes().for_each(|b| self.send(b)))
	}
}
