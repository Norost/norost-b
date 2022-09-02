use core::arch::asm;
pub use cpuid::Cpuid as Features;

mod cr0 {
	use super::*;

	pub fn get() -> u32 {
		let mut cr0: u32;
		unsafe { asm!("mov {:r}, cr0", out(reg) cr0, options(nostack, nomem, preserves_flags)) }
		cr0
	}

	pub unsafe fn set(cr0: u32) {
		unsafe { asm!("mov cr0, {:r}", in(reg) cr0, options(nostack, nomem, preserves_flags)) }
	}

	/// Monitor Co-Processor
	pub const MP: u32 = 1 << 1;
	/// Emulation
	pub const EM: u32 = 1 << 2;
	/// Task Switched
	pub const TS: u32 = 1 << 3;
}

mod cr4 {
	pub const OSFXSR: u32 = 1 << 9;
	pub const OSXMMEXCPT: u32 = 1 << 10;
	pub const FSGSBASE: u32 = 1 << 16;
	pub const OSXSAVE: u32 = 1 << 18;
}

pub fn try_enable_features(features: &Features) {
	let mut cr0 = cr0::get();
	cr0 &= !cr0::EM;
	cr0 |= cr0::MP;
	cr0 |= cr0::TS;
	unsafe { cr0::set(cr0) };

	let mut cr4: u32;
	unsafe { asm!("mov {:r}, cr4", out(reg) cr4, options(nostack, nomem, preserves_flags)) };
	// SSE is guaranteed to be supported on x86_64
	cr4 |= cr4::OSFXSR;
	cr4 |= cr4::OSXMMEXCPT;
	features.fsgsbase().then(|| cr4 |= cr4::FSGSBASE);
	features.osxsave().then(|| cr4 |= cr4::OSXSAVE);
	unsafe { asm!("mov cr4, {:r}", in(reg) cr4, options(nostack, nomem, preserves_flags)) };
}

/// Assert CR0.TS
pub fn mark_task_switch() {
	unsafe { cr0::set(cr0::get() | cr0::TS) };
}

/// Deassert CR0.TS
pub fn clear_task_switch() {
	unsafe { cr0::set(cr0::get() & !cr0::TS) };
}
