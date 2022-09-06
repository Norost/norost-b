#![no_std]
#![feature(alloc_error_handler)]
#![feature(new_uninit)]
#![feature(start)]

extern crate alloc;

mod rasterizer;

use {
	alloc::{collections::VecDeque, vec::Vec},
	core::time::Duration,
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	fontdue::{Font, FontSettings},
	io_queue_rt::{Pow2Size, Queue},
	rt::{Error, Handle},
};

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
	let mut scale = 20.0;
	let mut no_quit = false;

	let mut args = rt::args::args()
		.skip(1)
		.map(|s| core::str::from_utf8(s).unwrap());
	while let Some(a) = args.next() {
		match a {
			"--scale" => {
				scale = args
					.next()
					.expect("scale requires argument")
					.parse()
					.expect("invalid scale format")
			}
			"--no-quit" => no_quit = true,
			a => panic!("unknown arg {:?}", a),
		}
	}

	let font = Font::from_bytes(FONT, FontSettings { scale: 160.0, ..Default::default() }).unwrap();
	let mut rasterizer = rasterizer::Rasterizer::new(font, scale);

	let window = rt::args::handle(b"window").expect("window undefined");

	let mut res = [0; 8];
	let l = window
		.get_meta(b"bin/resolution".into(), (&mut res).into())
		.unwrap();
	let mut width = u32::from_le_bytes(res[..4].try_into().unwrap());
	let mut height = u32::from_le_bytes(res[4..l].try_into().unwrap());

	let new_fb = |w, h| {
		let (fb, _) = {
			let size = w as usize * h as usize * 3;
			let (fb, _) = rt::Object::new(rt::NewObject::SharedMemory { size }).unwrap();
			window
				.share(
					&rt::Object::new(rt::NewObject::PermissionMask {
						handle: fb.as_raw(),
						rwx: rt::io::RWX::R,
					})
					.unwrap()
					.0,
				)
				.unwrap();
			fb.map_object(None, rt::io::RWX::RW, 0, usize::MAX).unwrap()
		};
		unsafe { rasterizer::FrameBuffer::new(fb.cast(), w, h) }
	};
	let drop_fb = |fb: &mut rasterizer::FrameBuffer, w, h| {
		let size = (w as usize * h as usize * 3 + 0xfff) & !0xfff;
		unsafe { rt::mem::dealloc(fb.as_ptr().cast(), size).unwrap() };
	};

	let mut fb = new_fb(width, height);

	let mut draw = |fb: &mut rasterizer::FrameBuffer, rs: &mut rasterizer::Rasterizer, w, h| {
		fb.as_mut().fill(0);
		rs.render_all(fb);
		let draw = ipc_wm::Flush {
			origin: ipc_wm::Point { x: 0, y: 0 },
			size: ipc_wm::SizeInclusive { x: (w - 1) as _, y: (h - 1) as _ },
		};
		window.write(&draw.encode()).unwrap();
	};

	window
		.set_meta(b"bin/cmd/fill".into(), (&[0, 0, 0]).into())
		.unwrap();

	let (tbl_buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let table = StreamTable::new(&tbl_buf, rt::io::Pow2Size(4), (1 << 8) - 1);
	rt::args::handle(b"share")
		.expect("share undefined")
		.share(table.public())
		.expect("failed to share");

	const WRITE_HANDLE: Handle = Handle::MAX - 1;
	const READ_HANDLE: Handle = Handle::MAX - 2;

	let queue = Queue::new(Pow2Size::P6, Pow2Size::P6).unwrap();

	let mut poll_table = queue.submit_read(table.notifier().as_raw(), ()).unwrap();
	let mut poll_window = queue
		.submit_read(window.as_raw(), Vec::with_capacity(16))
		.unwrap();

	let mut read_jobs = VecDeque::new();

	let mut parser = Parser { state: ParserState::Idle };
	let mut next_draw = rt::time::Monotonic::MAX;
	loop {
		queue.poll();
		queue.wait(next_draw.duration_since(rt::time::Monotonic::now()));
		queue.process();

		let mut flush = false;

		if let Some((res, b)) = driver_utils::task::poll(&mut poll_window) {
			res.unwrap();
			match ipc_wm::Event::decode((&*b).try_into().unwrap()).unwrap() {
				ipc_wm::Event::Resize(r) => {
					drop_fb(&mut fb, width, height);
					width = r.x;
					height = r.y;
					fb = new_fb(width, height);
					next_draw = rt::time::Monotonic::ZERO;
				}
				ipc_wm::Event::Key(k) if k.is_press() => {
					if let scancodes::KeyCode::Unicode(c) = k.key() {
						if let Some(id) = read_jobs.pop_front() {
							let mut b = [0; 4];
							let b = c.encode_utf8(&mut b);
							let d = table.alloc(b.len()).expect("out of buffers");
							d.copy_from(0, b.as_bytes());
							table.enqueue(id, Response::Data(d));
							flush = true;
						} else {
							todo!();
						}
					}
				}
				ipc_wm::Event::Key(_) => {}
			}
			poll_window = queue.submit_read(window.as_raw(), b).unwrap();
		}

		if driver_utils::task::poll(&mut poll_table).is_some() {
			poll_table = queue.submit_read(table.notifier().as_raw(), ()).unwrap();
		}
		let next_draw_t = rt::time::Monotonic::now()
			.checked_add(Duration::from_millis(33))
			.unwrap_or(rt::time::Monotonic::ZERO);
		while let Some((handle, job_id, req)) = table.dequeue() {
			let resp = match req {
				Request::Open { path } => {
					let mut p = [0; 16];
					let p = &mut p[..path.len()];
					path.copy_to(0, p);
					match &*p {
						b"write" => Response::Handle(WRITE_HANDLE),
						b"read" => Response::Handle(READ_HANDLE),
						_ => Response::Error(Error::DoesNotExist),
					}
				}
				Request::Write { data } if handle == WRITE_HANDLE => {
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
					}

					let len = data.len().try_into().unwrap();
					next_draw = next_draw.min(next_draw_t);
					Response::Amount(len)
				}
				Request::Close => match handle {
					// Exit
					READ_HANDLE => continue,
					WRITE_HANDLE if no_quit => continue,
					// Exit immediately so we don't run *blocking* Drop of queue
					// TODO perhaps Queue shouldn't be blocking on drop?
					WRITE_HANDLE => rt::exit(0),
					_ => unreachable!(),
				},
				Request::Read { amount } if handle == READ_HANDLE => {
					read_jobs.push_back(job_id);
					continue;
				}
				_ => Response::Error(Error::InvalidOperation),
			};
			table.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| table.flush());
		if next_draw <= rt::time::Monotonic::now() {
			draw(&mut fb, &mut rasterizer, width, height);
			next_draw = rt::time::Monotonic::MAX;
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
