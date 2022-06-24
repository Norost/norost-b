#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::{ptr::NonNull, str};
use fontdue::{
	layout::{CoordinateSystem, Layout, TextStyle},
	Font, FontSettings, Metrics,
};
use rt::io::{Error, Handle};

const FONT: &[u8] = include_bytes!("../../../thirdparty/font/inconsolata/Inconsolata-VF.ttf");

#[cfg(not(test))]
#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let root = rt::io::file_root().unwrap();

	let fonts = &[Font::from_bytes(
		FONT,
		FontSettings {
			scale: 160.0,
			..Default::default()
		},
	)
	.unwrap()];

	let window = root.create(b"window_manager/window").unwrap();

	// Clear some area at the top so the text doesn't look ugly
	let mut raw = Vec::new();
	let mut draw = ipc_wm::DrawRect::new_vec(
		&mut raw,
		ipc_wm::Point { x: 0, y: 0 },
		ipc_wm::Size { x: 700, y: 320 },
	);
	draw.pixels_mut().fill(0);
	window.write_vec(raw, 0).unwrap();

	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
	layout.append(fonts, &TextStyle::new("ABC\n", 160.0, 0));
	layout.append(fonts, &TextStyle::new("Hello, world!", 40.0, 0));

	for glyph in layout.glyphs().iter() {
		if !glyph.char_data.rasterize() {
			continue;
		}
		let font = &fonts[glyph.font_index];

		let mut raw = Vec::new();
		let mut draw = ipc_wm::DrawRect::new_vec(
			&mut raw,
			ipc_wm::Point {
				x: glyph.x as _,
				y: glyph.y as _,
			},
			ipc_wm::Size {
				x: (glyph.width - 1) as _,
				y: (glyph.height - 1) as _,
			},
		);

		let (_, covmap) = font.rasterize_config(glyph.key);
		draw.pixels_mut()
			.chunks_exact_mut(3)
			.zip(covmap.iter())
			.for_each(|(w, &r)| w.copy_from_slice(&[r, r, r]));

		window.write_vec(draw.raw, 0).unwrap();
	}

	rt::thread::sleep(core::time::Duration::MAX);
	rt::thread::sleep(core::time::Duration::MAX);

	0
}
