#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use driver_utils::os::stream_table::{Request, Response, StreamTable};
use fontdue::{
	layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle, WrapStyle},
	Font, FontSettings,
};
use rt::{Error, Handle};

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

	let mut res = [0; 8];
	let l = window
		.get_meta(b"bin/resolution".into(), (&mut res).into())
		.unwrap();
	let width = u32::from_le_bytes(res[..4].try_into().unwrap());
	let height = u32::from_le_bytes(res[4..l].try_into().unwrap());

	window
		.set_meta(b"bin/cmd/fill".into(), (&[0, 0, 0]).into())
		.unwrap();

	let tbl_buf = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let mut table = StreamTable::new(&tbl_buf, rt::io::Pow2Size(5));
	root.create(b"gui_cli")
		.unwrap()
		.share(&table.public_table())
		.unwrap();

	const WRITE_HANDLE: Handle = Handle::MAX - 1;

	let mut cur_line = String::new();

	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
	layout.reset(&LayoutSettings {
		max_width: Some(width as _),
		max_height: Some(height as _),
		wrap_style: WrapStyle::Letter,
		..Default::default()
	});
	let mut raw = Vec::new();
	let mut draw_str = |s: &str, erase_from: usize, offt: usize| -> bool {
		layout.clear();
		layout.append(fonts, &TextStyle::new(s, 20.0, 0));
		if layout.height() > height as f32 {
			return true;
		}
		for (i, glyph) in layout
			.glyphs()
			.iter()
			.enumerate()
			.skip(offt.min(erase_from))
		{
			if !glyph.char_data.rasterize() {
				continue;
			}
			let font = &fonts[glyph.font_index];

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

			if i < erase_from {
				let (_, covmap) = font.rasterize_config(glyph.key);
				draw.pixels_mut()
					.chunks_exact_mut(3)
					.zip(covmap.iter())
					.for_each(|(w, &r)| w.copy_from_slice(&[r, r, r]));
			} else {
				draw.pixels_mut().fill(0);
			}

			window.write(&raw).unwrap();
		}
		false
	};

	let mut parser = Parser {
		string: Default::default(),
		state: ParserState::Idle,
	};
	loop {
		let mut flush = false;
		while let Some((handle, req)) = table.dequeue() {
			let (job_id, resp) = match req {
				Request::Open { job_id, path } => {
					let mut p = [0; 16];
					let p = &mut p[..path.len()];
					path.copy_to(0, p);
					let resp = match &*p {
						b"write" => Response::Handle(WRITE_HANDLE),
						_ => Response::Error(Error::DoesNotExist),
					};
					(job_id, resp)
				}
				Request::Write { job_id, data } => {
					let r = match handle {
						WRITE_HANDLE => {
							let og = parser.string.clone();
							let mut erase_from = og.len();

							let l = data.len().min(1024);
							let mut v = Vec::with_capacity(l);
							data.copy_to_uninit(0, &mut v.spare_capacity_mut()[..l]);
							unsafe { v.set_len(l) }
							for c in v {
								parser.add(c);
								erase_from = erase_from.min(parser.string.len());
							}

							let len = data.len().try_into().unwrap();
							draw_str(&og, erase_from, og.len());
							if draw_str(&parser.string, parser.string.len(), og.len()) {
								parser.remove_first_line();
								window
									.set_meta(b"bin/cmd/fill".into(), (&[0, 0, 0]).into())
									.unwrap();
								draw_str(&parser.string, parser.string.len(), 0);
							}
							Response::Amount(len)
						}
						_ => Response::Error(Error::InvalidOperation),
					};
					data.manual_drop();
					(job_id, r)
				}
				Request::Write { .. } => todo!(),
				Request::Close => match handle {
					WRITE_HANDLE => continue,
					_ => unreachable!(),
				},
				e => todo!(),
			};
			table.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| table.flush());
		table.wait();
	}
}

struct Parser {
	string: String,
	state: ParserState,
}

enum ParserState {
	Idle,
	AnsiEscape(AnsiState),
}

enum AnsiState {
	Start,
	BracketOpen,
	Erase,
}

impl Parser {
	fn add(&mut self, c: u8) {
		match &mut self.state {
			ParserState::Idle => match c {
				0x1b => self.state = ParserState::AnsiEscape(AnsiState::Start),
				0x7f => {
					self.string.pop();
				}
				c => {
					char::from_u32(c.into()).map(|c| self.string.push(c));
				}
			},
			ParserState::AnsiEscape(s) => match (s, c) {
				(s @ AnsiState::Start, b'[') => *s = AnsiState::BracketOpen,
				(s @ AnsiState::BracketOpen, b'2') => *s = AnsiState::Erase,
				(AnsiState::Erase, b'K') => {
					let i = self.string.rfind('\n').map_or(0, |i| i + 1);
					self.string.truncate(i);
					self.state = ParserState::Idle
				}
				_ => {
					self.string.push(char::REPLACEMENT_CHARACTER);
					self.state = ParserState::Idle
				}
			},
		}
	}

	fn remove_first_line(&mut self) {
		let i = self.string.find('\n').map_or(self.string.len(), |i| i + 1);
		self.string = self.string.split_off(i);
	}
}
