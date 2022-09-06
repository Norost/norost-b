#![feature(norostb)]

use std::{
	fs::File,
	io::{Cursor, Read, Write},
	os::norostb::prelude::*,
};

static EXAMPLE: &[u8] = include_bytes!("/tank/stupid_memes/rust_evangelism_strike_force.jpg");

fn main() {
	let mut window = File::create("window_manager/window").unwrap();
	let mut buf = [0; 8];
	rt::io::get_meta(
		window.as_handle(),
		b"bin/resolution".into(),
		(&mut buf).into(),
	)
	.unwrap();
	let mut res = ipc_wm::Resolution::decode(buf);

	loop {
		let (fb_ptr, fb_size) = {
			let (fb, _) = rt::Object::new(rt::NewObject::SharedMemory {
				size: res.x as usize * res.y as usize * 3,
			})
			.unwrap();
			let (fb_rdonly, _) = rt::Object::new(rt::NewObject::PermissionMask {
				handle: fb.as_raw(),
				rwx: rt::io::RWX::R,
			})
			.unwrap();
			rt::io::share(window.as_handle(), fb_rdonly.as_raw()).unwrap();
			fb.map_object(None, rt::io::RWX::RW, 0, usize::MAX).unwrap()
		};
		let fb = unsafe { std::slice::from_raw_parts_mut(fb_ptr.as_ptr(), fb_size) };

		let mut img = jpeg::Decoder::new(Cursor::new(EXAMPLE));
		let w = res.x.try_into().unwrap_or(u16::MAX);
		let h = res.y.try_into().unwrap_or(u16::MAX);
		let (img_w, img_h) = img.scale(w, h).unwrap();
		let img = img.decode().unwrap();

		for y in 0..usize::from(h) {
			for x in 0..usize::from(w) {
				let sx = x * usize::from(img_w) / usize::from(w);
				let sy = y * usize::from(img_h) / usize::from(h);
				let f = &img[(sy * usize::from(img_w) + sx) * 3..][..3];
				let t = &mut fb[(y * usize::from(w) + x) * 3..][..3];
				t.copy_from_slice(f);
			}
		}

		let draw = ipc_wm::Flush {
			origin: ipc_wm::Point { x: 0, y: 0 },
			size: ipc_wm::SizeInclusive { x: w - 1, y: h - 1 },
		};
		window.write_all(&draw.encode()).unwrap();

		drop(fb);
		unsafe { rt::mem::dealloc(fb_ptr, fb_size).unwrap() };

		let mut evt = [0; 16];
		let l = window.read(&mut evt).unwrap();
		match ipc_wm::Event::decode(evt[..l].try_into().unwrap()).unwrap() {
			ipc_wm::Event::Resize(r) => res = r,
			_ => {}
		}
	}
}
