pub struct Features {
	max_basic_eax: u32,
	max_extended_eax: u32,
}

macro_rules! flag {
	(basic $flag:ident = $id:literal | $bit:literal) => {
		pub fn $flag(&self) -> bool {
			let id: u32 = $id;
			// SAFETY: id is in range
			id <= self.max_basic_eax && unsafe { (CPUID::new(id).edx & 1 << $bit) != 0 }
		}
	};
	(extended $flag:ident = $id:literal | $bit:literal) => {
		pub fn $flag(&self) -> bool {
			let id: u32 = $id | 1 << 31;
			// SAFETY: id is in range
			id <= self.max_extended_eax && unsafe { (CPUID::new(id).edx & 1 << $bit) != 0 }
		}
	};
}

impl Features {
	pub fn new() -> Option<Self> {
		// Check if CPUID is available
		// SAFETY: all x86 chips support eflags
		unsafe {
			let out: u32;
			const FLAG_CPUID: u32 = 1 << 21;
			// Shamelessly stolen from https://wiki.osdev.org/CPUID#Checking_CPUID_availability
			//
			// How it works: flip bit, reload eflags, xor reloaded eflags with original eflags,
			// check if bit is non-zero.
			//
			// If the bit got changed, then 1 ^ 0 == 0 ^ 1 == 1 --> supported, otherwise
			// 0 ^ 0 == 1 ^ 1 == 0 --> not supported.
			asm!("
				pushfd
				pushfd
				xor		dword ptr [esp], {flag}
				popfd
				pushfd
				pop		{0}
				xor		{0}, [esp]
				popfd
			", out(reg) out, flag = const FLAG_CPUID);
			if out & FLAG_CPUID == 0 {
				return None;
			}
		}

		// Get maximum EAX value for basic and extended features
		// SAFETY: cpuid exists & using cpuid with eax = 0 and 0x8000_0000 is always safe.
		unsafe {
			let (max_basic_eax, max_extended_eax): (u32, u32);
			asm!("cpuid", inout("eax") 0 << 31 => max_basic_eax, out("ebx") _, out("ecx") _, out("edx") _);
			asm!("cpuid", inout("eax") 1 << 31 => max_extended_eax, out("ebx") _, out("ecx") _, out("edx") _);
			Some(Self {
				max_basic_eax,
				max_extended_eax,
			})
		}
	}

	// List stolen from https://sandpile.org/x86/cpuid.htm
	// Some like the elusive PG1G/pdpe1gb have been found in QEMU source (target/i386/cpu.c)
	flag!(basic mtrr = 0x1 | 12);
	flag!(extended pdpe1gb = 0x1 | 26);
}

struct CPUID {
	#[allow(dead_code)]
	eax: u32,
	#[allow(dead_code)]
	ebx: u32,
	#[allow(dead_code)]
	ecx: u32,
	edx: u32,
}

impl CPUID {
	// SAFETY: id has to be below basic_max or extended_max
	unsafe fn new(id: u32) -> Self {
		let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
		asm!("cpuid", inout("eax") id => eax, out("ebx") ebx, out("ecx") ecx, out("edx") edx);
		CPUID { eax, ebx, ecx, edx }
	}
}
