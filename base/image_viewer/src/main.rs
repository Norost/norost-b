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
	let mut img = jpeg::Decoder::new(Cursor::new(EXAMPLE));
	let (w, h) = img.scale(f(0, 4), f(4, 8)).unwrap();
	dbg!(w, h);
	dbg!(img.info());
	let img = img.decode().unwrap();

	let mut raw = Vec::new();
	ipc_wm::DrawRect::new_vec(
		&mut raw,
		ipc_wm::Point { x: 0, y: 0 },
		ipc_wm::Size { x: w - 1, y: h - 1 },
	)
	.pixels_mut()
	.copy_from_slice(&img);
	window.write_all(&raw).unwrap();

	loop {
		std::thread::sleep(std::time::Duration::MAX);
	}
}
