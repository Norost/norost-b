#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::mem;

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

	dev.draw(id, rect, &mut buf).expect("failed to draw");
	rt::thread::sleep(core::time::Duration::from_secs(5));
	dev.draw(id, rect, &mut buf).expect("failed to draw");
	rt::thread::sleep(core::time::Duration::from_secs(5));
	dev.draw(id, rect, &mut buf).expect("failed to draw");

	rt::thread::sleep(core::time::Duration::MAX);

	0
}
