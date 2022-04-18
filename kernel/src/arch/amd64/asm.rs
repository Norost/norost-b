pub mod io {
	use core::arch::asm;

	pub unsafe fn inb(address: u16) -> u8 {
		let out: u8;
		unsafe {
			asm!("in al, dx", out("al") out, in("dx") address);
		}
		out
	}

	pub unsafe fn outb(address: u16, value: u8) {
		unsafe {
			asm!("out dx, al", in("dx") address, in("al") value);
		}
	}
}
