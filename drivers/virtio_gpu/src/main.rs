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

macro_rules! log {
	($($arg:tt)*) => {{
		let _ = rt::io::stderr().map(|o| writeln!(o, $($arg)*));
	}};
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let table_name = rt::args::Args::new()
		.skip(1)
		.next()
		.expect("expected table path");

	let root = rt::io::file_root().unwrap();
	let it = root.open(b"pci/info").unwrap();
	let dev = loop {
		let e = it.read_vec(32).unwrap();
		if e.is_empty() {
			log!("no VirtIO GPU device found");
			return 1;
		}
		let s = core::str::from_utf8(&e).unwrap();
		let (loc, id) = s.split_once(' ').unwrap();
		if id == "1af4:1050" {
			let mut path = Vec::from(*b"pci/");
			path.extend(loc.as_bytes());
			break path;
		}
	};
	let dev = root.open(&dev).unwrap();
	let poll = dev.open(b"poll").unwrap();
	let pci = kernel::syscall::map_object(dev.as_raw(), None, 0, usize::MAX).unwrap();
	let pci = unsafe { pci::Pci::new(pci.cast(), 0, 0, &[]) };

	let dma_alloc = |size, _align| -> Result<_, ()> {
		let (d, _) = kernel::syscall::alloc_dma(None, size).unwrap();
		let a = kernel::syscall::physical_address(d).unwrap();
		Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
	};

	let mut dev = {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					kernel::syscall::map_object(dev.as_raw(), None, (bar + 1).into(), usize::MAX)
						.unwrap()
						.cast()
				};

				let msix = virtio_gpu::Msix {
					control: Some(0),
					cursor: Some(1),
				};

				unsafe { virtio_gpu::Device::new(h, map_bar, dma_alloc, msix).unwrap() }
			}
			_ => unreachable!(),
		}
	};

	// Allocate buffers for virtio queue requests
	let (buf, buf_size) =
		kernel::syscall::alloc_dma(None, 256).expect("failed to allocate framebuffer buffer");
	let buf_phys = kernel::syscall::physical_address(buf).unwrap();
	let mut buf = unsafe {
		virtio::PhysMap::new(
			buf.cast(),
			virtio::PhysAddr::new(buf_phys.try_into().unwrap()),
			buf_size.get(),
		)
	};
	let (buf2, buf2_size) =
		kernel::syscall::alloc_dma(None, 256).expect("failed to allocate framebuffer buffer");
	let buf2_phys = kernel::syscall::physical_address(buf2).unwrap();
	let buf2 = unsafe {
		virtio::PhysMap::new(
			buf2.cast(),
			virtio::PhysAddr::new(buf2_phys.try_into().unwrap()),
			buf2_size.get(),
		)
	};

	// Allocate draw buffer
	let (width, height) = (400, 300);
	let (fb, fb_size) = kernel::syscall::alloc_dma(None, width * height * 4)
		.expect("failed to allocate framebuffer buffer");
	let fb_phys = kernel::syscall::physical_address(fb).unwrap();
	let fb = unsafe {
		virtio::PhysMap::new(
			fb.cast(),
			virtio::PhysAddr::new(fb_phys.try_into().unwrap()),
			fb_size.get(),
		)
	};

	// Set up scanout
	let mut backing = virtio_gpu::BackingStorage::new(buf2);
	backing.push(&fb);
	let rect = virtio_gpu::Rect::new(0, 0, width.try_into().unwrap(), height.try_into().unwrap());
	let ret = unsafe { dev.init_scanout(virtio_gpu::Format::Rgbx8Unorm, rect, backing, &mut buf) };
	let id = ret.unwrap();

	// Draw colors
	for y in 0..height {
		for x in 0..width {
			let r = x * 255 / width;
			let g = y * 255 / height;
			let b = 255 - (r + g) / 2;
			unsafe {
				fb.virt()
					.cast::<[u8; 4]>()
					.as_ptr()
					.add(y * width + x)
					.write([r as u8, g as u8, b as u8, 255]);
			}
		}
	}

	// Wrap framebuffer for sharing
	let fb_share = rt::Object::new(rt::NewObject::MemoryMap {
		range: fb.virt()..=NonNull::new(fb.virt().as_ptr().wrapping_add(fb.size() - 1)).unwrap(),
	})
	.unwrap()
	.into_raw();

	dev.draw(id, rect, &mut buf).expect("failed to draw");

	// Create table
	let tbl = rt::io::file_root().unwrap().create(table_name).unwrap();

	// Begin event loop
	loop {
		let data = tbl.read_vec(64).unwrap();
		let resp = match Job::deserialize(&data).unwrap() {
			Job::Open {
				job_id,
				handle,
				path,
			} => match (handle, path) {
				(Handle::MAX, b"framebuffer") => {
					Job::reply_open_share_clear(data, job_id, fb_share)
				}
				(Handle::MAX, b"sync") => Job::reply_open_clear(data, job_id, 0),
				(Handle::MAX, _) => Job::reply_error_clear(data, job_id, Error::DoesNotExist),
				_ => Job::reply_error_clear(data, job_id, Error::InvalidOperation),
			},
			Job::Write {
				job_id,
				handle,
				data: d,
			} => match (handle, d) {
				(0, &[]) => {
					dev.draw(id, rect, &mut buf).expect("failed to draw");
					Job::reply_write_clear(data, job_id, 0)
				}
				_ => Job::reply_error_clear(data, job_id, Error::InvalidOperation),
			},
			Job::Close { .. } => continue,
			_ => todo!(),
		}
		.unwrap();
		tbl.write_vec(resp, 0).unwrap();
	}
}
