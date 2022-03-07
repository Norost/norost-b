use core::arch::asm;
use core::arch::x86_64::{CpuidResult, __cpuid, __cpuid_count};

mod cr4 {
	pub const FSGSBASE: u32 = 1 << 16;
}

pub fn enable_fsgsbase() {
	unsafe {
		asm!("
			mov {0}, cr4
			or {0}, {1}
			mov cr4, {0}
			",
			out(reg) _,
			const cr4::FSGSBASE,
		);
	}
}
