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
use core::{
	ptr::{self, NonNull},
	str,
};
use driver_utils::io::queue::stream::Job;
use math::{Point, Rect, Size};
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
	let sync = root.open(b"gpu/sync").unwrap();
	let (w, h) = {
		let r = root.open(b"gpu/resolution").unwrap();
		let r = r.read_vec(16).unwrap();
		let r = str::from_utf8(&r).unwrap();
		let (w, h) = r.split_once('x').unwrap();
		(w.parse::<u32>().unwrap(), h.parse::<u32>().unwrap())
	};
	let size = Size::new(w, h);

	let shmem_size = size.x as usize * size.y as usize * 3;
	let shmem_size = (shmem_size + 0xfff) & !0xfff;
	let mut shmem_obj =
		rt::io::new_object(rt::io::NewObject::SharedMemory { size: shmem_size }).unwrap();
	let shmem = rt::io::map_object(shmem_obj, None, 0, shmem_size).unwrap();
	sync.share(&rt::Object::from_raw(shmem_obj))
		.expect("failed to share mem with GPU");

	let gwp = window::GlobalWindowParams { border_width: 4 };
	let mut manager = manager::Manager::new(gwp).unwrap();
	let w0 = manager.new_window(size).unwrap();
	let w1 = manager.new_window(size).unwrap();
	let w2 = manager.new_window(size).unwrap();

	let sync_rect = |rect: math::Rect| {
		let mut s = alloc::string::String::with_capacity(64);
		use core::fmt::Write;
		let (l, h) = (rect.low(), rect.high());
		write!(&mut s, "{},{} {},{}", l.x, l.y, h.x, h.y).unwrap();
		sync.write(s.as_bytes()).unwrap();
	};
	let fill = |rect: math::Rect, color: [u8; 3]| {
		let s = rect.size().x as usize * rect.size().y as usize;
		assert!(s * 3 <= shmem_size, "TODO");
		for i in 0..s {
			unsafe {
				shmem.as_ptr().cast::<[u8; 3]>().add(i).write(color);
			}
		}
		sync_rect(rect);
	};

	let colors = [
		[255, 0, 0],
		[0, 255, 0],
		[0, 0, 255],
		[255, 255, 0],
		[0, 255, 255],
		[255, 0, 255],
	];

	fill(Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
	for (w, c) in manager.window_handles().zip(&colors) {
		fill(manager.window_rect(w, size).unwrap(), *c);
	}

	let table = root.create(b"window_manager").unwrap();

	loop {
		let buf = table.read_vec(1 << 20).unwrap();
		let buf = match Job::deserialize(&buf).unwrap() {
			Job::Create {
				handle,
				job_id,
				path,
			} => match (handle, path) {
				(Handle::MAX, b"window") => {
					let h = manager.new_window(size).unwrap();
					fill(Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
					for (w, c) in manager.window_handles().zip(&colors) {
						fill(manager.window_rect(w, size).unwrap(), *c);
					}
					Job::reply_create_clear(buf, job_id, h)
				}
				_ => Job::reply_error_clear(buf, job_id, Error::InvalidOperation),
			},
			Job::Write {
				handle,
				job_id,
				data,
			} => match handle {
				Handle::MAX => Job::reply_error_clear(buf, job_id, Error::InvalidOperation),
				h => {
					let display = Rect::from_size(Point::ORIGIN, size);
					let rect = manager.window_rect(h, size).unwrap();
					let draw = ipc_wm::DrawRect { raw: data.into() };
					let draw_size = draw.size().unwrap();
					// TODO do we actually want this?
					let draw_size = Size::new(
						(u32::from(draw_size.x) + 1).min(rect.size().x),
						(u32::from(draw_size.y) + 1).min(rect.size().y),
					);
					let draw_orig = draw.origin().unwrap();
					let draw_orig = Point::new(draw_orig.x, draw_orig.y);
					let draw_rect = rect
						.calc_global_pos(Rect::from_size(draw_orig, draw_size))
						.unwrap();
					debug_assert_eq!((0..draw_size.x).count(), draw_rect.x().count());
					debug_assert_eq!((0..draw_size.y).count(), draw_rect.y().count());
					let pixels = draw.pixels().unwrap();
					assert!(
						draw_rect.high().x * size.y as u32 + draw_rect.high().y <= size.x * size.y
					);
					// TODO we can avoid this copy by passing shared memory buffers directly
					// to the GPU
					unsafe {
						shmem
							.as_ptr()
							.copy_from_nonoverlapping(pixels.as_ptr(), pixels.len());
					}
					sync_rect(draw_rect);
					let l = data.len().try_into().unwrap();
					Job::reply_write_clear(buf, job_id, l)
				}
			},
			Job::Close { handle } => {
				manager.destroy_window(handle).unwrap();
				fill(Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
				for (w, c) in manager.window_handles().zip(&colors) {
					fill(manager.window_rect(w, size).unwrap(), *c);
				}
				continue;
			}
			_ => todo!(),
		}
		.unwrap();
		table.write_vec(buf, 0).unwrap();
	}

	0
}
