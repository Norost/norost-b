use core::arch::asm;

pub const IA32_APIC_BASE_MSR: u32 = 0x1b;
#[allow(dead_code)]
pub const IA32_APIC_BASE_MSR_BSP: u64 = 0x100;
pub const IA32_APIC_BASE_MSR_ENABLE: u64 = 1 << 11;

pub const IA32_EFER: u32 = 0xc0000080;
pub const IA32_EFER_SCE: u64 = 1;

#[allow(dead_code)]
pub const IA32_KERNEL_GS_BASE: u32 = 0xc0000102;

pub const STAR: u32 = 0xc0000081;
pub const LSTAR: u32 = 0xc0000082;

pub const FS_BASE: u32 = 0xc0000100;
pub const GS_BASE: u32 = 0xc0000101;
pub const KERNEL_GS_BASE: u32 = 0xc0000102;

pub unsafe fn wrmsr(reg: u32, value: u64) {
	let (high, low) = ((value >> 32) as u32, value as u32);
	asm!("wrmsr", in("ecx") reg, in("edx") high, in("eax") low);
}

pub unsafe fn rdmsr(reg: u32) -> u64 {
	let (high, low): (u32, u32);
	asm!("rdmsr", in("ecx") reg, out("edx") high, out("eax") low);
	u64::from(high) << 32 | u64::from(low)
}

pub unsafe fn set_bits(reg: u32, bits: u64, on: bool) {
	let mut msr = rdmsr(reg);
	msr &= !bits;
	msr |= bits * u64::from(u8::from(on));
	wrmsr(reg, msr);
}
