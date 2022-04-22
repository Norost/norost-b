#![no_std]
#![feature(allocator_api)]
#![feature(inline_const)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(slice_ptr_get, slice_ptr_len)]

pub mod alloc;
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
