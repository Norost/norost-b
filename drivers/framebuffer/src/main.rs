#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::string::ToString;
use core::ptr::NonNull;
use driver_utils::os::stream_table::{Request, Response, StreamTable};
use framebuffer::{Bgrx8888, FrameBuffer, Rgbx8888};
use rt::{Error, Handle};
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let fb = rt::args::handle(b"framebuffer").expect("framebuffer undefined");
	let share = rt::args::handle(b"share").expect("share undefined");
	let mut fb_info = [0; 13];
	let l = fb
		.get_meta(b"bin/info".into(), (&mut fb_info).into())
		.unwrap();
	assert!(l == fb_info.len());
	let stride = u16::from_le_bytes(fb_info[0..][..2].try_into().unwrap());
	let width = u16::from_le_bytes(fb_info[2..][..2].try_into().unwrap());
	let height = u16::from_le_bytes(fb_info[4..][..2].try_into().unwrap());
	let [bpp, r_pos, r_mask, g_pos, g_mask, b_pos, b_mask]: [u8; 7] =
		fb_info[6..].try_into().unwrap();

	assert_eq!((bpp, r_mask, g_mask, b_mask), (32, 8, 8, 8));

	let map_len = (stride as usize + 1) * (height as usize + 1);
	let (base, len) = fb
		.map_object(
			None,
			rt::RWX::RW,
			0,
			map_len,
			rt::io::MAP_OBJECT_HINT_NON_TEMPORAL,
		)
		.unwrap();
	assert!(len >= map_len);

	enum Fb {
		Rgbx8888(FrameBuffer<Rgbx8888>),
		Bgrx8888(FrameBuffer<Bgrx8888>),
	}
	let mut fb = unsafe {
		match (r_pos, g_pos, b_pos) {
			(0, 8, 16) => Fb::Rgbx8888(FrameBuffer::new(base.cast(), width, height, stride)),
			(16, 8, 0) => Fb::Bgrx8888(FrameBuffer::new(base.cast(), width, height, stride)),
			_ => panic!("unsupported pixel format"),
		}
	};

	let (tbl, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 8 }).unwrap();
	let tbl = StreamTable::new(&tbl, 64.try_into().unwrap(), 64.try_into().unwrap());
	share.create(b"gpu").unwrap().share(tbl.public()).unwrap();

	let mut command_buf = (NonNull::<u8>::dangling(), 0);

	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((handle, job_id, req)) = tbl.dequeue() {
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
						let r = ipc_gpu::Resolution {
							x: width as _,
							y: height as _,
						}
						.encode();
						let data = tbl.alloc(r.len()).unwrap();
						data.copy_from(0, &r);
						Response::Data(data)
					}
					_ => Response::Error(Error::DoesNotExist),
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
									(cmd.stride * 3 - 1) as _,
									cmd.origin.x as _,
									cmd.origin.y as _,
									(cmd.size.x - 1) as _,
									(cmd.size.y - 1) as _,
								),
								Fb::Bgrx8888(fb) => fb.copy_from_raw_untrusted_rgb24_to_bgrx32(
									command_buf.0.as_ptr().add(cmd.offset as _).cast(),
									(cmd.stride * 3 - 1) as _,
									cmd.origin.x as _,
									cmd.origin.y as _,
									(cmd.size.x - 1) as _,
									(cmd.size.y - 1) as _,
								),
							}
						}
						Response::Amount(d.len().try_into().unwrap())
					} else {
						Response::Error(Error::InvalidData as _)
					}
				}
				Request::Share { share } => {
					match share.map_object(None, rt::io::RWX::R, 0, 1 << 30, 0) {
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
