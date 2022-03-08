#![no_std]
#![feature(inline_const)]

pub mod tls;

use core::arch::global_asm;
pub use norostb_kernel as kernel;

cfg_if::cfg_if! {
	if #[cfg(target_arch = "x86_64")] {
		global_asm!(include_str!("x86_64.s"));
		mod x86_64;
		pub(crate) use x86_64::*;
	} else {
		compiler_error!("unsupported architecture");
	}
}
