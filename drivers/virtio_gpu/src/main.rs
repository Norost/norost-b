#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::{string::ToString, vec::Vec};
use core::{ptr::NonNull, str};
use driver_utils::os::stream_table::{Buffer, Request, Response};
use rt::io::{Error, Handle};
use virtio_gpu::Rect;

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
		let mut r = [0; 32];
		let l = it.read(&mut r).unwrap();
		if l == 0 {
			log!("no VirtIO GPU device found");
			return 1;
		}
		let s = str::from_utf8(&r[..l]).unwrap();
		let (loc, id) = s.split_once(' ').unwrap();
		if id == "1af4:1050" {
			let mut path = Vec::from(*b"pci/");
			path.extend(loc.as_bytes());
			break path;
		}
	};
	let dev = root.open(&dev).unwrap();
	let poll = dev.open(b"poll").unwrap();
	let pci = dev
		.map_object(None, rt::io::RWX::R, 0, usize::MAX)
		.unwrap()
		.0;
	let pci = unsafe { pci::Pci::new(pci.cast(), 0, 0, &[]) };

	let dma_alloc = |size: usize, _align| -> Result<_, ()> {
		let (d, a, _) = driver_utils::dma::alloc_dma(size.try_into().unwrap()).unwrap();
		Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
	};

	let mut dev = {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					assert!(bar < 6);
					let mut s = *b"bar0";
					s[3] += bar;
					dev.open(&s)
						.unwrap()
						.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
						.unwrap()
						.0
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
	let (buf, buf_phys, buf_size) = driver_utils::dma::alloc_dma(256.try_into().unwrap()).unwrap();
	let mut buf = unsafe {
		virtio::PhysMap::new(buf.cast(), virtio::PhysAddr::new(buf_phys), buf_size.get())
	};
	let (buf2, buf2_phys, buf2_size) =
		driver_utils::dma::alloc_dma(256.try_into().unwrap()).unwrap();
	let buf2 = unsafe {
		virtio::PhysMap::new(
			buf2.cast(),
			virtio::PhysAddr::new(buf2_phys),
			buf2_size.get(),
		)
	};

	// Allocate draw buffer
	let (width, height) = (2560, 1440);
	let (fb, fb_phys, fb_size) =
		driver_utils::dma::alloc_dma((width * height * 4).try_into().unwrap()).unwrap();
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
	let rect = Rect::new(0, 0, width.try_into().unwrap(), height.try_into().unwrap());
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

	// Create table
	let tbl_buf = rt::Object::new(rt::NewObject::SharedMemory { size: 4096 }).unwrap();
	let mut tbl = driver_utils::os::stream_table::StreamTable::new(&tbl_buf, 64);

	rt::io::file_root()
		.unwrap()
		.create(table_name)
		.unwrap()
		.share(&tbl.public_table())
		.unwrap();

	// Sync doesn't need any storage, so optimize it a little by using a constant handle.
	const SYNC_HANDLE: Handle = Handle::MAX - 1;

	let mut handles = driver_utils::Arena::new();
	let mut command_buf = (NonNull::new(kernel::Page::SIZE as *mut u8).unwrap(), 0);

	enum H {
		Resolution,
	}

	// Begin event loop
	loop {
		let mut send_notif = false;
		while let Some((handle, flags, req)) = tbl.dequeue() {
			let (job_id, response) = match req {
				Request::Open { job_id, path } => (job_id, {
					let mut p = [0; 64];
					let p = &mut p[..path.len()];
					path.copy_to(0, p);
					path.manual_drop();
					match (handle, &*p) {
						(Handle::MAX, b"sync") => Response::Handle(SYNC_HANDLE),
						(Handle::MAX, b"resolution") => {
							Response::Handle(handles.insert(H::Resolution))
						}
						(Handle::MAX, _) => Response::Error(Error::DoesNotExist as _),
						_ => Response::Error(Error::InvalidOperation as _),
					}
				}),
				Request::Read { job_id, amount } => (
					job_id,
					match handle {
						Handle::MAX | SYNC_HANDLE => Response::Error(Error::InvalidOperation as _),
						h => {
							let buf = tbl.alloc(64).unwrap();
							let l = match &mut handles[h] {
								H::Resolution => {
									if flags.binary() {
										buf.copy_from(0, &(width as u32).to_le_bytes());
										buf.copy_from(4, &(height as u32).to_le_bytes());
										8
									} else {
										let (w, h) = (width.to_string(), height.to_string());
										buf.copy_from(0, w.as_bytes());
										buf.copy_from(w.len(), &[b'x']);
										buf.copy_from(w.len() + 1, h.as_bytes());
										(w.len() + 1 + h.len()).try_into().unwrap()
									}
								}
							};
							Response::Data {
								data: buf,
								length: l,
							}
						}
					},
				),
				Request::Write { job_id, data } => {
					let mut d = [0; 64];
					let d = &mut d[..data.len()];
					data.copy_to(0, d);
					data.manual_drop();
					let r = match handle {
						// Blit a specific area
						SYNC_HANDLE => {
							if flags.binary() {
								if let &mut [xl0, xl1, yl0, yl1, xh0, xh1, yh0, yh1] = d {
									let f = |l, h| u32::from(u16::from_le_bytes([l, h]));
									let (x0, y0, x1, y1) =
										(f(xl0, xl1), f(yl0, yl1), f(xh0, xh1), f(yh0, yh1));
									let (xl, yl) = (x0.min(x1), y0.min(y1));
									let (xh, yh) = (x0.max(x1), y0.max(y1));
									let r = Rect::new(xl, yl, xh - xl, yh - yl);
									dev.draw(id, r, &mut buf).expect("failed to draw");
									Response::Amount(d.len().try_into().unwrap())
								} else {
									Response::Error(Error::InvalidData as _)
								}
							} else {
								if let Some(r) = (|| {
									let s = str::from_utf8(d).ok()?;
									let f = |n: &str| n.parse::<u32>().ok();
									let (l, h) = s.split_once(' ')?;
									let (xl, yl) = l.split_once(',')?;
									let (xh, yh) = h.split_once(',')?;
									let (x0, y0, x1, y1) = (f(xl)?, f(yl)?, f(xh)?, f(yh)?);
									let (xl, yl) = (x0.min(x1), y0.min(y1));
									let (xh, yh) = (x0.max(x1), y0.max(y1));
									Some(Rect::new(xl, yl, xh - xl + 1, yh - yl + 1))
								})() {
									let area = r.height() as usize * r.width() as usize;
									assert!(area * 4 <= fb.size());
									assert!(area * 3 <= command_buf.1);
									unsafe {
										fb.virt().as_ptr().write_bytes(200, fb.size());
										for (fy, ty) in (0..r.height()).map(|h| (h, h)) {
											for (fx, tx) in (0..r.width()).map(|w| (w, w)) {
												let fi =
													fy as usize * r.width() as usize + fx as usize;
												// QEMU uses the stride of the *host* for the *guest*
												// memory too. Don't ask me why, this is documented literally
												// nowhere.
												// This, by the way, is the *only* reason we're forced to
												// allocate a framebuffer matching the host size.
												let ti = ty as usize * width as usize + tx as usize;
												let [r, g, b] = *command_buf
													.0
													.as_ptr()
													.cast::<[u8; 3]>()
													.add(fi);
												fb.virt()
													.as_ptr()
													.cast::<[u8; 4]>()
													.add(ti)
													.write([r, g, b, 0]);
											}
										}
									}
									dev.draw(id, r, &mut buf).expect("failed to draw");
									Response::Amount(d.len().try_into().unwrap())
								} else {
									Response::Error(Error::InvalidData as _)
								}
							}
						}
						_ => Response::Error(Error::InvalidOperation as _),
					};
					(job_id, r)
				}
				Request::Share { job_id, share } => (
					job_id,
					match handle {
						SYNC_HANDLE => match share.map_object(None, rt::io::RWX::R, 0, 1 << 30) {
							Err(e) => Response::Error(e as _),
							Ok((buf, size)) => {
								command_buf = (buf.cast(), size);
								Response::Amount(0)
							}
						},
						_ => Response::Error(Error::InvalidOperation as _),
					},
				),
				Request::Close => match handle {
					Handle::MAX | SYNC_HANDLE => continue,
					h => {
						handles.remove(h).unwrap();
						continue;
					}
				},
				Request::Create { job_id, path } => {
					path.manual_drop();
					(job_id, Response::Error(Error::InvalidOperation as _))
				}
				Request::Destroy { job_id, .. } | Request::Seek { job_id, .. } => {
					(job_id, Response::Error(Error::InvalidOperation as _))
				}
			};
			tbl.enqueue(job_id, response);
			send_notif = true;
		}
		send_notif.then(|| tbl.flush());
		tbl.wait();
	}
}
