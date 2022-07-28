pub mod io {
	use core::arch::asm;

	pub unsafe fn in8(address: u16) -> u8 {
		let out: u8;
		unsafe { asm!("in al, dx", out("al") out, in("dx") address) }
		out
	}

	pub unsafe fn in16(address: u16) -> u16 {
		let out: u16;
		unsafe { asm!("in ax, dx", out("ax") out, in("dx") address) }
		out
	}

	pub unsafe fn in32(address: u16) -> u32 {
		let out: u32;
		unsafe { asm!("in eax, dx", out("eax") out, in("dx") address) }
		out
	}

	pub unsafe fn out8(address: u16, value: u8) {
		unsafe { asm!("out dx, al", in("dx") address, in("al") value) }
	}

	pub unsafe fn out16(address: u16, value: u16) {
		unsafe { asm!("out dx, ax", in("dx") address, in("ax") value) }
	}

	pub unsafe fn out32(address: u16, value: u32) {
		unsafe { asm!("out dx, eax", in("dx") address, in("eax") value) }
	}
}
