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
		dbg!(v);
		v.try_into().unwrap()
	};
	let (w, h) = (f(0, 4), f(4, 8));
	let mut img = jpeg::Decoder::new(Cursor::new(EXAMPLE));
	let (img_w, img_h) = img.scale(f(0, 4), f(4, 8)).unwrap();
	dbg!(img.info());
	let img = img.decode().unwrap();

	let mut raw = Vec::new();
	let mut draw = ipc_wm::DrawRect::new_vec(
		&mut raw,
		ipc_wm::Point { x: 0, y: 0 },
		ipc_wm::Size { x: w, y: h - 1 },
	);

	for y in 0..usize::from(h) {
		for x in 0..usize::from(w) {
			let sx = x * usize::from(img_w) / usize::from(w);
			let sy = y * usize::from(img_h) / usize::from(h);
			let f = &img[(sy * usize::from(img_w) + sx) * 3..][..3];
			let t = &mut draw.pixels_mut()[(y * usize::from(w) + x) * 3..][..3];
			t.copy_from_slice(f);
		}
	}
	window.write_all(&raw).unwrap();

	loop {
		std::thread::sleep(std::time::Duration::MAX);
	}
}
