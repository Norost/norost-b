#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::ptr::NonNull;
use driver_utils::io::queue::stream::Job;
use rt::io::{Error, Handle};

#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let root = rt::io::file_root().unwrap();
	let fb = root.open(b"framebuffer").unwrap();
	let fb = rt::io::map_object(fb.as_raw(), None, 0, usize::MAX)
		.unwrap()
		.cast::<[u8; 4]>();
	let sync = root.open(b"sync").unwrap();
	let (w, h) = (400, 300);
	for y in 0..h {
		for x in 0..w {
			let r = x * x;
			let g = y * y;
			unsafe {
				fb.as_ptr().add(y * w + x).write([r as u8, g as u8, 0, 0]);
			}
		}
	}
	rt::io::stderr().unwrap().write(b"a");
	rt::thread::sleep(core::time::Duration::from_secs(5));
	// FIXME wakes way too early.
	rt::io::stderr().unwrap().write(b"b");
	rt::thread::sleep(core::time::Duration::from_secs(5));
	rt::io::stderr().unwrap().write(b"c");
	sync.write(b"").unwrap();
	0
}
