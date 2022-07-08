#![no_std]
#![feature(alloc_error_handler)]
#![feature(new_uninit)]
#![feature(start)]

extern crate alloc;

mod rasterizer;

use alloc::{boxed::Box, string::String, vec::Vec};
use core::ptr::NonNull;
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
fn alloc_error(layout: core::alloc::Layout) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "allocation error for {:?}", layout));
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
	let font = Font::from_bytes(
		FONT,
		FontSettings {
			scale: 160.0,
			..Default::default()
		},
	)
	.unwrap();
	let mut rasterizer = rasterizer::Rasterizer::new(font, 20);

	let window = root.create(b"window_manager/window").unwrap();

	let mut res = [0; 8];
	let l = window
		.get_meta(b"bin/resolution".into(), (&mut res).into())
		.unwrap();
	let width = u32::from_le_bytes(res[..4].try_into().unwrap());
	let height = u32::from_le_bytes(res[4..l].try_into().unwrap());

	let (fb, _) = {
		let size = width as usize * height as usize * 3;
		let fb = rt::Object::new(rt::NewObject::SharedMemory { size }).unwrap();
		window
			.share(
				&rt::Object::new(rt::NewObject::PermissionMask {
					handle: fb.as_raw(),
					rwx: rt::io::RWX::R,
				})
				.unwrap(),
			)
			.unwrap();
		fb.map_object(None, rt::io::RWX::RW, 0, usize::MAX).unwrap()
	};
	let mut fb = unsafe { rasterizer::FrameBuffer::new(fb.cast(), width, height) };

	let mut draw = |rasterizer: &mut rasterizer::Rasterizer| {
		fb.as_mut().fill(0);
		rasterizer.render_all(&mut fb);
		let draw = ipc_wm::Flush {
			origin: ipc_wm::Point { x: 0, y: 0 },
			size: ipc_wm::SizeInclusive {
				x: (width - 1) as _,
				y: (height - 1) as _,
			},
		};
		window.write(&draw.encode()).unwrap();
	};

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

	let mut parser = Parser {
		state: ParserState::Idle,
	};
	let mut flushed @ mut dirty = false;
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
							let l = data.len().min(1024);
							let mut v = Vec::with_capacity(l);
							data.copy_to_uninit(0, &mut v.spare_capacity_mut()[..l]);
							unsafe { v.set_len(l) }
							for c in v {
								match parser.add(c) {
									None => continue,
									Some(Action::PushChar(c)) => rasterizer.push_char(c),
									Some(Action::PopChar) => rasterizer.pop_char(),
									Some(Action::NewLine) => rasterizer.new_line(),
									Some(Action::ClearLine) => rasterizer.clear_line(),
								}
								dirty = true;
							}

							let len = data.len().try_into().unwrap();
							Response::Amount(len)
						}
						_ => Response::Error(Error::InvalidOperation),
					};
					data.manual_drop();
					(job_id, r)
				}
				Request::Write { .. } => todo!(),
				Request::Close => match handle {
					// Exit
					WRITE_HANDLE => return 0,
					_ => unreachable!(),
				},
				e => todo!(),
			};
			table.enqueue(job_id, resp);
			flush = true;
		}
		if flush {
			table.flush();
			flushed = true;
		} else if flushed {
			// TODO lazy hack, add some kind of table.wait_until instead (i.e. use
			// async I/O queue).
			for i in 0..10 {
				rt::thread::yield_now();
			}
			flushed = false;
		} else if dirty {
			draw(&mut rasterizer);
			dirty = false;
		} else {
			table.wait();
		}
	}
}

struct Parser {
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

enum Action {
	PushChar(char),
	PopChar,
	NewLine,
	ClearLine,
}

impl Parser {
	fn add(&mut self, c: u8) -> Option<Action> {
		match &mut self.state {
			ParserState::Idle => match c {
				0x1b => {
					self.state = ParserState::AnsiEscape(AnsiState::Start);
					None
				}
				0x7f => Some(Action::PopChar),
				b'\n' => Some(Action::NewLine),
				c => char::from_u32(c.into()).map(Action::PushChar),
			},
			ParserState::AnsiEscape(s) => match (s, c) {
				(s @ AnsiState::Start, b'[') => {
					*s = AnsiState::BracketOpen;
					None
				}
				(s @ AnsiState::BracketOpen, b'2') => {
					*s = AnsiState::Erase;
					None
				}
				(AnsiState::Erase, b'K') => {
					self.state = ParserState::Idle;
					Some(Action::ClearLine)
				}
				_ => {
					self.state = ParserState::Idle;
					Some(Action::PushChar(char::REPLACEMENT_CHARACTER))
				}
			},
		}
	}
}
