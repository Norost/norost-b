use core::arch::asm;

pub const IA32_PAT: u32 = 0x277;

/// # Safety
///
/// MSRs must be supported.
pub unsafe fn rdmsr(msr: u32) -> u64 {
	let (hi, lo): (u32, u32);
	asm!("rdmsr", in("ecx") msr, lateout("edx") hi, lateout("eax") lo, options(nomem, nostack));
	(u64::from(hi) << 32) | u64::from(lo)
}

/// # Safety
///
/// MSRs must be supported.
pub unsafe fn wrmsr(reg: u32, value: u64) {
	let (high, low) = ((value >> 32) as u32, value as u32);
	unsafe {
		asm!("wrmsr", in("ecx") reg, in("edx") high, in("eax") low, options(nostack, nomem));
	}
}
