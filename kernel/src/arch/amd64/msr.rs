use core::arch::asm;

pub const IA32_APIC_BASE_MSR: u32 = 0x1b;
#[allow(dead_code)]
pub const IA32_APIC_BASE_MSR_BSP: u64 = 0x100;
pub const IA32_APIC_BASE_MSR_ENABLE: u64 = 1 << 11;

pub const IA32_EFER: u32 = 0xc0000080;
pub const IA32_EFER_SCE: u64 = 1;

#[allow(dead_code)]
pub const IA32_KERNEL_GS_BASE: u32 = 0xc0000102;

/// Ring 0 and Ring 3 Segment bases, as well as SYSCALL EIP in protected mode.
pub const STAR: u32 = 0xc0000081;
/// The kernel's RIP SYSCALL entry in long mode.
pub const LSTAR: u32 = 0xc0000082;
/// The kernel's RIP for SYSCALL in compatibility mode.
#[allow(dead_code)]
pub const CSTAR: u32 = 0xc0000083;
/// The low 32 bits are the SYSCALL flag mask. If a bit in this is set, the corresponding bit in
/// RFLAGS is cleared.
pub const SFMASK: u32 = 0xc0000084;

pub const FS_BASE: u32 = 0xc0000100;
pub const GS_BASE: u32 = 0xc0000101;
pub const KERNEL_GS_BASE: u32 = 0xc0000102;

pub unsafe fn wrmsr(reg: u32, value: u64) {
	let (high, low) = ((value >> 32) as u32, value as u32);
	unsafe {
		asm!("wrmsr", in("ecx") reg, in("edx") high, in("eax") low, options(nostack, nomem));
	}
}

pub unsafe fn rdmsr(reg: u32) -> u64 {
	let (high, low): (u32, u32);
	unsafe {
		asm!("rdmsr", in("ecx") reg, out("edx") high, out("eax") low, options(nostack, nomem));
	}
	u64::from(high) << 32 | u64::from(low)
}

pub unsafe fn set_bits(reg: u32, bits: u64, on: bool) {
	unsafe {
		let mut msr = rdmsr(reg);
		msr &= !bits;
		msr |= bits * u64::from(u8::from(on));
		wrmsr(reg, msr);
	}
}
