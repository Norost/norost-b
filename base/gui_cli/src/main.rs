#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::{ptr::NonNull, str};
use rt::io::{Error, Handle};

#[cfg(not(test))]
#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let root = rt::io::file_root().unwrap();

	let window = root.create(b"window_manager/window").unwrap();

	rt::thread::sleep(core::time::Duration::MAX);
	rt::thread::sleep(core::time::Duration::MAX);

	0
}
