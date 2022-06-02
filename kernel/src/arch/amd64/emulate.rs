//! Emulate unsupported instructions.
//!
//! Emulated instructions:
//! - `wrfsbase r32` (`f3` `0f` `ae` `d0`)
//! - `wrfsbase r64` (`f3` `REX.W` `0f` `ae` `d0`)
//! - `wrgsbase r32` (`f3` `0f` `ae` `d8`)
//! - `wrgsbase r64` (`f3` `REX.W` `0f` `ae` `d8`)
//! - `rdfsbase r32` (`f3` `0f` `ae` `c0`)
//! - `rdfsbase r64` (`f3` `REX.W` `0f` `ae` `c0`)
//! - `rdgsbase r32` (`f3` `0f` `ae` `c8`)
//! - `rdgsbase r64` (`f3` `REX.W` `0f` `ae` `c8`)
//!
//! REX format:
//! ```
//! 0100 WRXB
//! ```
//! - `W`: 64-bit if 1
//! - `R`: modrm.reg extension
//! - `X`: sib.index extension
//! - `B`: modrm.rm or sib.base extension
//!
//! ModR/M format:
//! ```
//! 7:6 5:2 2:0
//! mod reg rm
//! ```

use super::msr;

#[naked]
unsafe extern "C" fn handle_invalid_opcode() {
	unsafe {
		core::arch::asm!(
			"push r15",
			"push r14",
			"push r13",
			"push r12",
			"push r11",
			"push r10",
			"push r9",
			"push r8",
			"push rdi",
			"push rsi",
			"push rbp",
			"push rdi", // Push a dummy, as rsp is actually the kernel stack
			"push rbx",
			"push rdx",
			"push rcx",
			"push rax",
			"lea rdi, [rsp + 16 * 8]",
			"lea rdx, [rsp + 16 * 11]",
			"mov rsi, rsp",
			"call {decode}",
			"pop rax",
			"pop rcx",
			"pop rdx",
			"pop rbx",
			"pop rdi", // Ditto
			"pop rbp",
			"pop rsi",
			"pop rdi",
			"pop r8",
			"pop r9",
			"pop r10",
			"pop r11",
			"pop r12",
			"pop r13",
			"pop r14",
			"pop r15",
			"iretq",
			decode = sym decode,
			options(noreturn),
		);
	}
}

struct ModRM(pub u8);

impl ModRM {
	#[allow(dead_code)]
	fn reg(&self) -> u8 {
		(self.0 >> 3) & 7
	}

	fn rm(&self) -> u8 {
		self.0 & 7
	}
}

struct Rex(pub u8);

impl Rex {
	#[allow(dead_code)]
	fn reg(&self, modrm: &ModRM) -> u8 {
		modrm.reg() | ((self.0 & 0b0100) << 1)
	}

	fn rm(&self, modrm: &ModRM) -> u8 {
		modrm.rm() | ((self.0 & 0b0001) << 3)
	}

	fn is_64(&self) -> bool {
		self.0 & 0b1000 != 0
	}
}

struct Decoder<'a> {
	rip: &'a mut *const u8,
	regs: &'a mut [u64; 16],
	rsp: &'a mut u64,
}

impl Decoder<'_> {
	fn get_reg(&self, n: u8) -> u64 {
		if n == 4 {
			*self.rsp
		} else {
			self.regs[usize::from(n)]
		}
	}

	fn set_reg(&mut self, n: u8, reg: u64) {
		if n == 4 {
			*self.rsp = reg
		} else {
			self.regs[usize::from(n)] = reg
		}
	}

	fn fsgs64_common(&mut self, i: u64) -> (u8, bool) {
		let modrm = ModRM((i >> 32) as u8);
		let rex = Rex((i >> 8) as u8);
		*self.rip = self.rip.wrapping_add(5);
		(rex.rm(&modrm), rex.is_64())
	}

	fn fsgs64_read(&mut self, i: u64, msr: u32) {
		let (reg, w) = self.fsgs64_common(i);
		let v = unsafe { msr::rdmsr(msr) };
		self.set_reg(
			reg,
			match w {
				true => v,
				false => v & 0xffff_ffff,
			},
		);
	}

	fn fsgs64_write(&mut self, i: u64, msr: u32) {
		let (reg, w) = self.fsgs64_common(i);
		let v = self.get_reg(reg);
		unsafe {
			msr::wrmsr(
				msr,
				match w {
					true => v,
					false => v & 0xffff_ffff,
				},
			)
		};
	}
}

/// # Note
///
/// No swapgs has been performed.
extern "C" fn decode(rip: &mut *const u8, regs: &mut [u64; 16], rsp: &mut u64) {
	super::disable_interrupts();
	// FIXME ensure we're loading from a valid page
	let instr = unsafe { (*rip).cast::<u64>().read_unaligned() };
	let mut dec = Decoder { rip, regs, rsp };
	match instr {
		// (wr|rd)(fs|gs)base (r64)
		i if i & 0xd8_ff_ff_f6_ff == 0xc0_ae_0f_40_f3 => dec.fsgs64_read(i, msr::FS_BASE),
		i if i & 0xd8_ff_ff_f6_ff == 0xc8_ae_0f_40_f3 => dec.fsgs64_read(i, msr::GS_BASE),
		i if i & 0xd8_ff_ff_f6_ff == 0xd0_ae_0f_40_f3 => dec.fsgs64_write(i, msr::FS_BASE),
		i if i & 0xd8_ff_ff_f6_ff == 0xd8_ae_0f_40_f3 => dec.fsgs64_write(i, msr::GS_BASE),
		_ => {
			fatal!("Invalid opcode!");
			fatal!("  RIP:     {:?}", *rip);
			// TODO notify user thread somehow.
			loop {
				super::halt();
			}
		}
	}
}

pub(super) unsafe fn init() {
	use super::*;
	unsafe {
		idt_set(
			6,
			IDTEntry::new(gdt::GDT::KERNEL_CS, handle_invalid_opcode, 0),
		);
	}
}
