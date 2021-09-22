/// # Safety
///
/// MSRs must be supported.
pub unsafe fn rdmsr(msr: u32) -> u64 {
	let (hi, lo): (u32, u32);
	asm!("rdmsr", in("ecx") msr, lateout("edx") hi, lateout("eax") lo);
	(u64::from(hi) << 32) | u64::from(lo)
}
