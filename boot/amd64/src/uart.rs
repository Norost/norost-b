use core::{arch::asm, fmt};

const PORT: u16 = 0x3f8;

unsafe fn inb(address: u16) -> u8 {
	let out: u8;
	asm!("in al, dx", out("al") out, in("dx") address);
	out
}

unsafe fn outb(address: u16, value: u8) {
	asm!("out dx, al", in("dx") address, in("al") value);
}

#[must_use = "I/O port space accesses cannot be optimized out"]
fn line_status() -> u8 {
	unsafe { inb(PORT + 5) }
}

#[must_use = "I/O port space accesses cannot be optimized out"]
fn transmit_empty() -> bool {
	line_status() & (1 << 6) != 0
}

pub struct Uart;

impl Uart {
	pub fn send(byte: u8) {
		while !transmit_empty() {}
		unsafe { outb(PORT, byte) }
	}
}

impl fmt::Write for Uart {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		Ok(s.bytes().for_each(Self::send))
	}
}
