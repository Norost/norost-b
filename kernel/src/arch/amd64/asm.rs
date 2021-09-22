pub mod io {
	pub unsafe fn inb(address: u16) -> u8 {
		let out: u8;
		asm!("in al, dx", out("al") out, in("dx") address);
		out
	}

	pub unsafe fn outb(address: u16, value: u8) {
		asm!("out dx, al", in("dx") address, in("al") value);
	}
}
