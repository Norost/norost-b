use crate::arch::amd64::asm::io;
use core::fmt;

pub struct UART {
	port: u16,
}

impl UART {
	pub unsafe fn new(port: u16) -> Self {
		// TODO
		Self { port }
	}

	pub fn send(&mut self, byte: u8) {
		while !self.transmit_empty() {}
		unsafe { io::outb(self.port, byte) }
	}
	
	#[must_use = "I/O port space accesses cannot be optimized out"]
	pub fn transmit_empty(&self) -> bool {
		self.line_status() & 1 << 6 != 0
	}

	#[must_use = "I/O port space accesses cannot be optimized out"]
	fn line_status(&self) -> u8 {
		unsafe { io::inb(self.port + 5) }
	}
}

impl fmt::Write for UART {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		Ok(s.bytes().for_each(|b| self.send(b)))
	}
}
