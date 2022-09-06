#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use {
	alloc::string::ToString,
	core::ptr::NonNull,
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	rt::io::{Error, Handle},
	virtio_gpu::Rect,
};

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
	let table_name = rt::args::args()
		.skip(1)
		.next()
		.expect("expected table path");

	let dev = rt::args::handle(b"pci").expect("pci undefined");
	let poll = dev.open(b"poll").unwrap();
	let (pci, l) = dev.map_object(None, rt::io::RWX::R, 0, usize::MAX).unwrap();
	assert!(l == 4096);
	let pci = unsafe { pci::Pci::new(pci.cast(), 0, l, &[]) };

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

				let msix = virtio_gpu::Msix { control: Some(0), cursor: Some(1) };

				unsafe { virtio_gpu::Device::new(h, map_bar, dma_alloc, msix).unwrap() }
			}
			_ => unreachable!(),
		}
	};
	let wait = || poll.read(&mut []).unwrap();
	let wait_tk = |dev: &mut virtio_gpu::Device, tk| {
		while dev.poll_control_queue(|t| assert_eq!(tk, t)) == 0 {
			wait();
		}
	};
	let wait_tk2 = |dev: &mut virtio_gpu::Device, tk| {
		while dev.poll_cursor_queue(|t| assert_eq!(tk, t)) == 0 {
			wait();
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
	let (buf3, buf3_phys, buf3_size) =
		driver_utils::dma::alloc_dma(256.try_into().unwrap()).unwrap();
	let buf3 = unsafe {
		virtio::PhysMap::new(
			buf3.cast(),
			virtio::PhysAddr::new(buf3_phys),
			buf3_size.get(),
		)
	};

	// Allocate draw buffer
	let (width, height) = (1920, 1080);
	let (fb, fb_phys, fb_size) =
		driver_utils::dma::alloc_dma((width * height * 4).try_into().unwrap()).unwrap();
	let fb = unsafe {
		virtio::PhysMap::new(
			fb.cast(),
			virtio::PhysAddr::new(fb_phys.try_into().unwrap()),
			fb_size.get(),
		)
	};

	let (cursor, cursor_phys, cursor_size) =
		driver_utils::dma::alloc_dma((64 * 64 * 4).try_into().unwrap()).unwrap();
	let cursor = unsafe {
		virtio::PhysMap::new(
			cursor.cast(),
			virtio::PhysAddr::new(cursor_phys.try_into().unwrap()),
			cursor_size.get(),
		)
	};

	// Set up scanout
	let mut backing = virtio_gpu::BackingStorage::new(buf2);
	let mut cursor_backing = virtio_gpu::BackingStorage::new(buf3);
	backing.push(&fb);
	cursor_backing.push(&cursor);
	let scanout_id = 0;
	let scanout_resource_id = 1.try_into().unwrap();
	let cursor_resource_id = 2.try_into().unwrap();

	let rect = Rect::new(0, 0, width.try_into().unwrap(), height.try_into().unwrap());
	unsafe {
		let tk = dev
			.create_resource_2d(
				scanout_resource_id,
				rect,
				virtio_gpu::Format::Rgbx8Unorm,
				&mut buf,
			)
			.unwrap();
		wait_tk(&mut dev, tk);
		let tk = dev
			.attach_resource_2d(scanout_resource_id, backing, &mut buf)
			.unwrap();
		wait_tk(&mut dev, tk);
		let tk = dev
			.init_scanout(scanout_id, scanout_resource_id, rect, &mut buf)
			.unwrap();
		wait_tk(&mut dev, tk);
	}

	unsafe {
		let rect = Rect::new(0, 0, 64, 64);
		let tk = dev
			.create_resource_2d(
				cursor_resource_id,
				rect,
				virtio_gpu::Format::Rgba8Unorm,
				&mut buf,
			)
			.unwrap();
		wait_tk(&mut dev, tk);
		let tk = dev
			.attach_resource_2d(cursor_resource_id, cursor_backing, &mut buf)
			.unwrap();
		wait_tk(&mut dev, tk);
	}

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

	unsafe {
		let tk = dev
			.transfer(scanout_resource_id, rect, &mut buf)
			.expect("failed to draw");
		wait_tk(&mut dev, tk);
		let tk = dev
			.flush(scanout_resource_id, rect, &mut buf)
			.expect("failed to draw");
		wait_tk(&mut dev, tk);
	}

	// Create table
	let (tbl_buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 4096 }).unwrap();
	let tbl = StreamTable::new(&tbl_buf, 64.try_into().unwrap(), 1024 - 1);

	rt::io::file_root()
		.unwrap()
		.create(table_name)
		.unwrap()
		.share(tbl.public())
		.unwrap();

	let mut command_buf = (NonNull::new(kernel::Page::SIZE as *mut u8).unwrap(), 0);

	// Begin event loop
	let mut tiny_buf = [0; 32];
	loop {
		let mut send_notif = false;
		while let Some((handle, job_id, req)) = tbl.dequeue() {
			let response = match req {
				Request::GetMeta { property } => {
					let prop = property.get(&mut tiny_buf);
					match &*prop {
						b"resolution" => {
							let (w, h) = (width.to_string(), height.to_string());
							let data = tbl.alloc(w.len() + 1 + h.len()).expect("out of buffers");
							data.copy_from(0, w.as_bytes());
							data.copy_from(w.len(), &[b'x']);
							data.copy_from(w.len() + 1, h.as_bytes());
							Response::Data(data)
						}
						b"bin/resolution" => {
							let r = ipc_gpu::Resolution { x: width as _, y: height as _ }.encode();
							let data = tbl.alloc(r.len()).unwrap();
							data.copy_from(0, &r);
							Response::Data(data)
						}
						_ => Response::Error(Error::DoesNotExist),
					}
				}
				Request::SetMeta { property_value } => match property_value.try_get(&mut [0; 32]) {
					Ok((b"bin/cursor/pos", &mut [a, b, c, d])) => {
						/*
						let x = u16::from_le_bytes([a, b]);
						let y = u16::from_le_bytes([c, d]);
						unsafe {
							let tk = dev
								.move_cursor(0, cursor_resource_id, x.into(), y.into(), &mut buf)
								.unwrap();
							wait_tk2(&mut dev, tk);
						}
						*/
						Response::Amount(0)
					}
					Ok((b"bin/cursor/pos", _)) => Response::Error(Error::InvalidData),
					Ok(_) => Response::Error(Error::DoesNotExist),
					Err(_) => Response::Error(Error::InvalidData),
				},
				Request::Write { data } => {
					let mut d = [0; 64];
					let d = &mut d[..data.len()];
					data.copy_to(0, d);
					// Blit a specific area
					if let Ok(d) = d.try_into() {
						let cmd = ipc_gpu::Flush::decode(d);
						assert_eq!(cmd.offset, 0, "todo: offset");
						assert_eq!(cmd.stride, u32::from(cmd.size.x), "todo: stride");
						let r = Rect::new(
							cmd.origin.x,
							cmd.origin.y,
							cmd.size.x.into(),
							cmd.size.y.into(),
						);
						let area = r.height() as usize * r.width() as usize;
						assert!(area * 4 <= fb.size());
						assert!(area * 3 <= command_buf.1);
						unsafe {
							fb.virt().as_ptr().write_bytes(200, fb.size());
							for (fy, ty) in (0..r.height()).map(|h| (h, h)) {
								for (fx, tx) in (0..r.width()).map(|w| (w, w)) {
									let fi = fy as usize * r.width() as usize + fx as usize;
									// QEMU uses the stride of the *host* for the *guest*
									// memory too. Don't ask me why, this is documented literally
									// nowhere.
									// This, by the way, is the *only* reason we're forced to
									// allocate a framebuffer matching the host size.
									let ti = ty as usize * width as usize + tx as usize;
									let [r, g, b] =
										*command_buf.0.as_ptr().cast::<[u8; 3]>().add(fi);
									fb.virt()
										.as_ptr()
										.cast::<[u8; 4]>()
										.add(ti)
										.write([r, g, b, 0]);
								}
							}
						}
						unsafe {
							let tk = dev
								.transfer(scanout_resource_id, r, &mut buf)
								.expect("failed to draw");
							wait_tk(&mut dev, tk);
							let tk = dev
								.flush(scanout_resource_id, r, &mut buf)
								.expect("failed to draw");
							wait_tk(&mut dev, tk);
						}
						Response::Amount(d.len().try_into().unwrap())
					} else if let Ok([0xc5, w, h]) = <[u8; 3]>::try_from(&*d) {
						rt::dbg!();
						let l = (usize::from(w) + 1) * (usize::from(h) + 1);
						if l * 4 <= command_buf.1 {
							unsafe {
								let r = Rect::new(0, 0, 64, 64);

								cursor.virt().as_ptr().write_bytes(0, 64 * 64 * 4);
								for y in 0..usize::from(h) + 1 {
									let t = cursor.virt().as_ptr().add(64 * 4 * y);
									let f =
										command_buf.0.as_ptr().add((usize::from(w) + 1) * 4 * y);
									t.copy_from_nonoverlapping(f, (usize::from(w) + 1) * 4);
								}
								let tk = dev.transfer(cursor_resource_id, r, &mut buf).unwrap();
								wait_tk(&mut dev, tk);
								let tk = dev.flush(cursor_resource_id, r, &mut buf).unwrap();
								wait_tk(&mut dev, tk);

								let tk = dev
									.update_cursor(0, cursor_resource_id, 0, 0, 0, 0, &mut buf)
									.unwrap();
								wait_tk2(&mut dev, tk);
							}
							Response::Amount(l as _)
						} else {
							Response::Error(Error::InvalidData)
						}
					} else {
						Response::Error(Error::InvalidData as _)
					}
				}
				Request::Share { share } => {
					match share.map_object(None, rt::io::RWX::R, 0, 1 << 30) {
						Err(e) => Response::Error(e as _),
						Ok((buf, size)) => {
							command_buf = (buf.cast(), size);
							Response::Amount(0)
						}
					}
				}
				Request::Close => continue,
				_ => Response::Error(Error::InvalidOperation as _),
			};
			tbl.enqueue(job_id, response);
			send_notif = true;
		}
		send_notif.then(|| tbl.flush());
		tbl.wait();
	}
}
