//! # Tiling window manager
//!
//! This window manager is based on binary trees: each leaf is a window and each node is
//! grouped per two by a parent up to the root.
//!
//! ## Node paths
//!
//! A path has any of the the following syntaxes:
//!
//! ```
//! <workspace id/name>:<window id>
//! ```

#![cfg_attr(not(test), no_std)]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

mod manager;
mod math;
mod window;
mod workspace;

use alloc::vec::Vec;
use core::{ptr::NonNull, str};
use driver_utils::io::queue::stream::Job;
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
	let fb = root.open(b"gpu/framebuffer").unwrap();
	let fb = rt::io::map_object(fb.as_raw(), None, 0, usize::MAX)
		.unwrap()
		.cast::<[u8; 4]>();
	let sync = root.open(b"gpu/sync").unwrap();
	let (w, h) = {
		let r = root.open(b"gpu/resolution").unwrap();
		let r = r.read_vec(16).unwrap();
		let r = str::from_utf8(&r).unwrap();
		let (w, h) = r.split_once('x').unwrap();
		(w.parse::<u32>().unwrap(), h.parse::<u32>().unwrap())
	};
	let size = math::Size::new(w, h);

	let gwp = window::GlobalWindowParams { border_width: 4 };
	let mut manager = manager::Manager::new(gwp).unwrap();
	let w0 = manager.new_window(size).unwrap();
	let w1 = manager.new_window(size).unwrap();
	let w2 = manager.new_window(size).unwrap();

	let mut fill = |rect: math::Rect, color: [u8; 3]| {
		writeln!(rt::io::stderr().unwrap(), "{:?}", rect);
		for y in rect.y() {
			for x in rect.x() {
				let x = usize::try_from(x).unwrap();
				let y = usize::try_from(y).unwrap();
				let w = usize::try_from(w).unwrap();
				unsafe {
					fb.as_ptr()
						.add(y * w + x)
						.write([color[0], color[1], color[2], 0]);
				}
			}
		}
	};

	fill(manager.window_rect(w0, size).unwrap(), [255, 0, 0]);
	fill(manager.window_rect(w1, size).unwrap(), [0, 255, 0]);
	fill(manager.window_rect(w2, size).unwrap(), [0, 0, 255]);

	sync.write(b"").unwrap();

	0
}
