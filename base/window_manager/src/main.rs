#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::{ptr::NonNull, str};
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
	let (w, h) = {
		let r = root.open(b"resolution").unwrap();
		let r = r.read_vec(16).unwrap();
		let r = str::from_utf8(&r).unwrap();
		let (w, h) = r.split_once('x').unwrap();
		(w.parse::<u32>().unwrap(), h.parse::<u32>().unwrap())
	};
	let (w, h) = (w.try_into().unwrap(), h.try_into().unwrap());
	for y in 0..h {
		for x in 0..w {
			let r = x * x;
			let g = y * y;
			unsafe {
				fb.as_ptr().add(y * w + x).write([r as u8, g as u8, 0, 0]);
			}
		}
	}
	// FIXME wakes way too early.
	rt::thread::sleep(core::time::Duration::from_secs(5));
	sync.write(b"40,40 80,80").unwrap();
	rt::thread::sleep(core::time::Duration::from_secs(1));
	rt::thread::sleep(core::time::Duration::from_secs(1));
	sync.write(b"120,120 80,80").unwrap();
	rt::thread::sleep(core::time::Duration::from_secs(1));
	rt::thread::sleep(core::time::Duration::from_secs(1));
	sync.write(b"").unwrap();
	0
}
