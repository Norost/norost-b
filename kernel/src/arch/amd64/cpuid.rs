use core::arch::asm;

mod cr4 {
	pub const FSGSBASE: u32 = 1 << 16;
}

pub struct Features {
	max_basic_eax: u32,
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
	//flag!(basic mtrr = 0x1 | edx[12]);
	flag!(basic fsgsbase = 0x7 | ebx[0]);
	//flag!(extended pdpe1gb = 0x1 | edx[26]);
}

struct Cpuid {
	#[allow(dead_code)]
	eax: u32,
	ebx: u32,
	#[allow(dead_code)]
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
				"mov r11d, ebx",
				"cpuid",
				"mov ebx, r11d",
				inout("eax") id => eax,
				out("r11d") ebx,
				out("ecx") ecx,
				out("edx") edx,
				options(nostack, nomem, preserves_flags, pure)
			);
		}
		Self { eax, ebx, ecx, edx }
	}
}

pub fn try_enable_fsgsbase(features: &Features) {
	if features.fsgsbase() {
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
}
