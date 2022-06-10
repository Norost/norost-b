// FIXME we need to be *very* careful with globals: https://github.com/rust-lang/cargo/issues/2363
//
// This is a big deal due to the rustc-dep-of-std feature being necessary at the moment.
//
// This is *very* annoying since we'll have to either:
// - move global things to a crate that doesn't even use *libcore*. This isn't a practical option.
// - ensure globals point to the same objects somehow. This can be achieved with weak linking.
// The latter option has been tried with the #[linkage] attribute but Rust is being a giant PITA
// and seems inconsistent in what types it allows for weak linking. global_asm! does work for our
// needs though it is easy to misuse.

#![no_std]
#![feature(allocator_api)]
#![feature(asm_const, asm_sym)]
#![feature(core_intrinsics)]
#![feature(const_btree_new)]
#![feature(inline_const)]
#![feature(linkage)]
#![feature(let_else)]
#![feature(maybe_uninit_slice, maybe_uninit_write_slice)]
#![feature(naked_functions)]
#![feature(new_uninit)]
#![feature(ptr_metadata)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate compiler_builtins;

pub mod args;
mod globals;
pub mod io;
pub mod process;
pub mod sync;
pub mod table;
pub mod thread;
pub mod tls;

use core::ptr::NonNull;

pub use norostb_kernel::{error::Error, time, AtomicHandle, Handle};
pub use process::Process;
pub use table::{Object, RefObject};

cfg_if::cfg_if! {
	if #[cfg(target_arch = "x86_64")] {
		mod x86_64;
		pub(crate) use x86_64::*;
	} else {
		compiler_error!("unsupported architecture");
	}
}

/// Set up the runtime.
///
/// # Safety
///
/// This may only be called once at the very start of the program.
unsafe extern "C" fn rt_start(arguments: Option<NonNull<u8>>) -> ! {
	unsafe {
		tls::init();
		io::init(arguments);
		args::init(arguments);
	}
	// SAFETY: we can't actually guarantee safety due to weak linkage.
	let status = unsafe { rt_main(arguments) };
	norostb_kernel::syscall::exit(status)
}

/// Do some preparatory work, then call the main function.
///
/// # Safety
///
/// This may only be called once.
///
/// # Note
///
/// This is weakly linked so it can be adapted as necessary for any programming language.
///
/// The default implementation is tailored for Rust's standard library.
///
/// To override it, export a strong symbol with the name `__rt_main`.
#[linkage = "weak"]
#[export_name = "__rt_main"]
unsafe extern "C" fn rt_main(_arguments: Option<NonNull<u8>>) -> i32 {
	extern "C" {
		fn main(argc: isize, argv: Option<NonNull<*const u8>>) -> i32;
	}
	// We don't use any of the parameters in stdlib but I haven't figured out how to
	// get rid of them yet :(
	unsafe { main(0, None) }
}

#[inline]
pub fn exit(code: i32) -> ! {
	norostb_kernel::syscall::exit(code)
}
