#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::{ptr::NonNull, str};
use fontdue::{Font, FontSettings, Metrics};
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

	let mut font = LazyFont::new(FONT);

	let window = root.create(b"window_manager/window").unwrap();
	let window = root.create(b"window_manager/window").unwrap();

	let s = "Hello";
	let mut x_offt = 0;
	let y_offt = 50;

	for c in s.chars() {
		let c = font.get(c);

		let mut draw = ipc_wm::DrawRect::new(Vec::new());
		draw.set_origin(ipc_wm::Point {
			x: x_offt,
			y: y_offt - u32::from(c.height()),
		});
		draw.set_size(ipc_wm::Size {
			x: c.width() - 1,
			y: c.height() - 1,
		});
		draw.pixels_mut()
			.unwrap()
			.chunks_exact_mut(3)
			.zip(c.iter())
			.for_each(|(w, ((_, _), r))| w.copy_from_slice(&[r, r, r]));

		window.write_vec(draw.raw, 0).unwrap();
		x_offt += u32::from(c.width());
	}

	rt::thread::sleep(core::time::Duration::MAX);
	rt::thread::sleep(core::time::Duration::MAX);
	rt::thread::sleep(core::time::Duration::from_secs(2));
	drop(window);
	rt::thread::sleep(core::time::Duration::from_secs(2));
	rt::thread::sleep(core::time::Duration::from_secs(2));

	0
}

/// Lazily render & cache characters at a fixed size so drawing them to the window is a simple
/// copy operation yet doesn't require rendering the entire Unicode charset.
struct LazyFont {
	font: Font,
	chars: BTreeMap<char, Char>,
}

impl LazyFont {
	pub fn new(data: &[u8]) -> Self {
		Self {
			font: Font::from_bytes(data, FontSettings::default()).unwrap(),
			chars: Default::default(),
		}
	}

	pub fn get(&mut self, c: char) -> &Char {
		self.chars.entry(c).or_insert_with(|| {
			let (m, c) = self.font.rasterize(c, 48.0);
			Char {
				width: m.width.try_into().unwrap(),
				height: m.height.try_into().unwrap(),
				covmap: c.into(),
			}
		})
	}
}

struct Char {
	width: u16,
	height: u16,
	covmap: Box<[u8]>,
}

impl Char {
	#[inline(always)]
	pub fn width(&self) -> u16 {
		self.width
	}

	#[inline(always)]
	pub fn height(&self) -> u16 {
		self.height
	}

	pub fn iter(&self) -> impl Iterator<Item = ((usize, usize), u8)> + '_ {
		(0..self.height)
			.flat_map(|y| (0..self.width).map(move |x| (x.into(), y.into())))
			.zip(self.covmap.iter().copied())
	}
}
