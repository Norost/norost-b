#![no_std]

use core::arch::global_asm;
pub use norostb_kernel as kernel;

cfg_if::cfg_if! {
	if #[cfg(target_arch = "x86_64")] {
		global_asm!(include_str!("x86_64.s"));
	} else {
		compiler_error!("unsupported architecture");
	}
}
