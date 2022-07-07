#![feature(norostb)]

use std::{fs::File, io::Cursor, io::Write, os::norostb::prelude::*};

static EXAMPLE: &[u8] = include_bytes!("/tank/stupid_memes/rust_evangelism_strike_force.jpg");

fn main() {
	let mut window = File::create("window_manager/window").unwrap();
	let mut res = [0; 8];
	rt::io::get_meta(
		window.as_handle(),
		b"bin/resolution".into(),
		(&mut res).into(),
	)
	.unwrap();
	let f = |a, b| {
		let v = u32::from_le_bytes(res[a..b].try_into().unwrap());
		v.try_into().unwrap()
	};
	let (w, h) = (f(0, 4), f(4, 8));

	let (fb, fb_size) = {
		let fb = rt::Object::new(rt::NewObject::SharedMemory {
			size: usize::from(w) * usize::from(h) * 3,
		})
		.unwrap();
		let fb_rdonly = rt::Object::new(rt::NewObject::PermissionMask {
			handle: fb.as_raw(),
			rwx: rt::io::RWX::R,
		})
		.unwrap();
		rt::io::share(window.as_handle(), fb_rdonly.as_raw()).unwrap();
		fb.map_object(None, rt::io::RWX::RW, 0, usize::MAX).unwrap()
	};
	let fb = unsafe { std::slice::from_raw_parts_mut(fb.as_ptr(), fb_size) };

	let mut img = jpeg::Decoder::new(Cursor::new(EXAMPLE));
	let (img_w, img_h) = img.scale(f(0, 4), f(4, 8)).unwrap();
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

	loop {
		std::thread::sleep(std::time::Duration::MAX);
	}
}
