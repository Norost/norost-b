use core::arch::asm;

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

pub struct Features {
	max_basic_eax: u32,
	#[allow(dead_code)]
	max_extended_eax: u32,
}

macro_rules! flag {
	(basic $flag:ident = $id:literal | $reg:ident[$bit:literal]) => {
		pub fn $flag(&self) -> bool {
			let id: u32 = $id;
			// SAFETY: id is in range
			id <= self.max_basic_eax && unsafe { (Cpuid::new(id).$reg & 1 << $bit) != 0 }
		}
	};
	(extended $flag:ident = $id:literal | $reg:ident[$bit:literal]) => {
		pub fn $flag(&self) -> bool {
			let id: u32 = $id | 1 << 31;
			// SAFETY: id is in range
			id <= self.max_extended_eax && unsafe { (Cpuid::new(id).$reg & 1 << $bit) != 0 }
		}
	};
}

impl Features {
	pub fn new() -> Self {
		// Get maximum EAX value for basic and extended features
		// SAFETY: cpuid is guaranteed to exist in long mode & using cpuid with eax = 0 and
		// 0x8000_0000 is always safe.
		unsafe {
			Self {
				max_basic_eax: Cpuid::new(0 << 31).eax,
				max_extended_eax: Cpuid::new(1 << 31).eax,
			}
		}
	}

	// List stolen from https://sandpile.org/x86/cpuid.htm
	flag!(basic osxsave = 0x1 | ecx[26]);
	flag!(basic fsgsbase = 0x7 | ebx[0]);
	flag!(basic avx2 = 0x7 | ebx[5]);
}

struct Cpuid {
	eax: u32,
	ebx: u32,
	ecx: u32,
	#[allow(dead_code)]
	edx: u32,
}

impl Cpuid {
	// SAFETY: id has to be below basic_max or extended_max
	unsafe fn new(id: u32) -> Self {
		let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
		unsafe {
			asm!(
				// Thanks LLVM for forbidding me from using ebx
				"mov r11, rbx",
				"cpuid",
				"xchg rbx, r11",
				inout("eax") id => eax,
				// TODO sublevels
				inout("ecx") 0 => ecx,
				out("r11d") ebx,
				out("edx") edx,
				options(nostack, nomem, preserves_flags, pure)
			);
		}
		Self { eax, ebx, ecx, edx }
	}
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
