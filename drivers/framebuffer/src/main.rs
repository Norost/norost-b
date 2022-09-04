#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::boxed::Box;

use {
	alloc::string::ToString,
	core::{
		ptr::NonNull,
		sync::atomic::{AtomicU32, Ordering},
		time::Duration,
	},
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	framebuffer::{Bgrx8888, FrameBuffer, Rgbx8888},
	rt::{sync::Mutex, Error},
	rt_default as _,
};

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let fb = rt::args::handle(b"framebuffer").expect("framebuffer undefined");
	let share = rt::args::handle(b"share").expect("share undefined");
	let mut fb_info = [0; 15];
	let l = fb
		.get_meta(b"bin/info".into(), (&mut fb_info).into())
		.unwrap();
	assert!(l == fb_info.len());
	let stride = u32::from_le_bytes(fb_info[0..][..4].try_into().unwrap());
	let width = u16::from_le_bytes(fb_info[4..][..2].try_into().unwrap());
	let height = u16::from_le_bytes(fb_info[6..][..2].try_into().unwrap());
	let [bpp, r_pos, r_mask, g_pos, g_mask, b_pos, b_mask]: [u8; 7] =
		fb_info[8..].try_into().unwrap();

	assert_eq!((bpp, r_mask, g_mask, b_mask), (32, 8, 8, 8));

	let map_len = stride as usize * (height as usize + 1);
	let (base, len) = fb.map_object(None, rt::RWX::RW, 0, map_len).unwrap();
	assert!(len >= map_len);

	// Encoding doesn't matter, really
	let mut back_fb = unsafe { FrameBuffer::<Rgbx8888>::new(base.cast(), width, height, stride) };

	let fb_stride = (u32::from(width) + 1) * 4;
	let fb_len = fb_stride as usize * (usize::from(height) + 1);
	let (fb_ptr, _) = rt::mem::alloc(None, fb_len, rt::RWX::RW).unwrap();
	enum Fb {
		Rgbx8888(FrameBuffer<Rgbx8888>),
		Bgrx8888(FrameBuffer<Bgrx8888>),
	}
	let mut fb = unsafe {
		match (r_pos, g_pos, b_pos) {
			(0, 8, 16) => Fb::Rgbx8888(FrameBuffer::new(fb_ptr.cast(), width, height, fb_stride)),
			(16, 8, 0) => Fb::Bgrx8888(FrameBuffer::new(fb_ptr.cast(), width, height, fb_stride)),
			_ => panic!("unsupported pixel format"),
		}
	};

	let (tbl, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 8 }).unwrap();
	let tbl = StreamTable::new(&tbl, 64.try_into().unwrap(), 64.try_into().unwrap());
	share.create(b"gpu").unwrap().share(tbl.public()).unwrap();

	let mut command_buf = (NonNull::<u8>::dangling(), 0);

	// AtomicU32 is more efficient than AtomicBool on some architectures (e.g. RISC-V).
	static CHANGES: AtomicU32 = AtomicU32::new(0);
	static CURSOR: Mutex<Cursor> = Mutex::new(Cursor { x: 0, y: 0, w: 0, h: 0, img: [0; 64 * 64] });

	struct Cursor {
		x: u16,
		y: u16,
		w: u8,
		h: u8,
		img: [i32; 64 * 64],
	}

	let huh = rt::thread::Thread::new(
		1 << 10,
		Box::new(move || loop {
			let next_t = rt::time::Monotonic::now()
				.checked_add(Duration::from_secs(1) / 60)
				.unwrap();
			// TODO implement some sort of semaphore to only wake this thread when necessary.
			// Right now this thread wakes 60 times per second, which isn't very efficient.
			let changes = CHANGES.fetch_and(0, Ordering::Acquire);
			if changes & 1 != 0 {
				// Flush the entire screen
				//
				// TODO investigate methods to reduce the amount of data copied without adding
				// excessive overhead.
				unsafe {
					back_fb.copy_from_raw_untrusted_32(
						fb_ptr.cast().as_ptr(),
						fb_stride,
						0,
						0,
						width,
						height,
					)
				}
			}
			if changes & 3 != 0 {
				// Draw the cursor
				let c = CURSOR.lock();
				if c.x < width && c.y < height {
					unsafe {
						back_fb.copy_from_raw_untrusted_32(
							c.img.as_ptr(),
							u32::from(c.w + 1) * 4,
							c.x,
							c.y,
							u16::from(c.w).min(width - c.x),
							u16::from(c.h).min(height - c.y),
						)
					}
				}
			}
			if let Some(t) = next_t.checked_duration_since(rt::time::Monotonic::now()) {
				rt::thread::sleep(t);
			}
		}),
	)
	.unwrap();

	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((_, job_id, req)) = tbl.dequeue() {
			let resp = match req {
				Request::GetMeta { property } => match &*property.get(&mut [0; 64]) {
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
				},
				Request::SetMeta { property_value } => match property_value.try_get(&mut [0; 64]) {
					Ok((b"bin/cursor/pos", v)) if v.len() == 4 => {
						let x = u16::from_le_bytes([v[0], v[1]]);
						let y = u16::from_le_bytes([v[2], v[3]]);
						let mut c = CURSOR.lock();
						(c.x, c.y) = (x, y);
						drop(c);
						CHANGES.fetch_or(2, Ordering::Release);
						Response::Amount(0)
					}
					Ok((b"bin/cursor/pos", v)) => Response::Error(Error::InvalidData),
					Ok(_) => Response::Error(Error::DoesNotExist),
					Err(_) => Response::Error(Error::InvalidData),
				},
				Request::Write { data } => {
					let mut buf = [0; 64];
					let (d, _) = data.copy_into(&mut buf);
					// Blit a specific area
					if let Ok(d) = d.try_into() {
						let cmd = ipc_gpu::Flush::decode(d);
						assert!(cmd.stride != 0 && cmd.size.x != 0 && cmd.size.y != 0);
						unsafe {
							match &mut fb {
								Fb::Rgbx8888(fb) => fb.copy_from_raw_untrusted_rgb24_to_rgbx32(
									command_buf.0.as_ptr().add(cmd.offset as _).cast(),
									cmd.stride * 3,
									cmd.origin.x as _,
									cmd.origin.y as _,
									(cmd.size.x - 1) as _,
									(cmd.size.y - 1) as _,
								),
								Fb::Bgrx8888(fb) => fb.copy_from_raw_untrusted_rgb24_to_bgrx32(
									command_buf.0.as_ptr().add(cmd.offset as _).cast(),
									cmd.stride * 3,
									cmd.origin.x as _,
									cmd.origin.y as _,
									(cmd.size.x - 1) as _,
									(cmd.size.y - 1) as _,
								),
							}
						}
						CHANGES.store(1, Ordering::Release);
						Response::Amount(d.len().try_into().unwrap())
					} else if let Ok([0xc5, w, h]) = <[u8; 3]>::try_from(&*d) {
						let l = (usize::from(w) + 1) * (usize::from(h) + 1);
						if l * 4 <= command_buf.1 {
							let mut c = CURSOR.lock();
							// FIXME untrusted
							unsafe {
								command_buf
									.0
									.as_ptr()
									.cast::<i32>()
									.copy_to_nonoverlapping(c.img.as_mut_ptr(), l);
							}
							(c.w, c.h) = (w, h);
							drop(c);
							CHANGES.fetch_or(2, Ordering::Release);
							Response::Amount(l as _)
						} else {
							Response::Error(Error::InvalidData)
						}
					} else {
						Response::Error(Error::InvalidData)
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
				_ => Response::Error(Error::InvalidOperation),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
	}
}
