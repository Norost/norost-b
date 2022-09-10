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

	// Spawn process
	let (process, inp, out) = {
		let bin = rt::args::handle(b"spawn").expect("spawn undefined");
		let (inp, p_inp) = rt::Object::new(rt::NewObject::Pipe).unwrap();
		let (p_out, out) = rt::Object::new(rt::NewObject::Pipe).unwrap();
		let mut b = rt::process::Builder::new().unwrap();
		b.set_binary(&bin).unwrap();
		b.add_object(b"in", &p_inp).unwrap();
		b.add_object(b"out", &p_out).unwrap();
		b.add_object(b"err", &p_out).unwrap();
		b.add_args(rt::args::args().take(1)).unwrap();
		(b.spawn().unwrap(), inp, out)
	};

	let font = {
		let font = rt::args::handle(b"font").expect("font undefined");
		let font = font.read_file_all().unwrap();
		Font::from_bytes(&*font, FontSettings { scale: 160.0, ..Default::default() }).unwrap()
	};
	let mut rasterizer = rasterizer::Rasterizer::new(font, scale);

	let window = rt::args::handle(b"window").expect("window undefined");

	window
		.set_meta(b"title".into(), b"Terminal".into())
		.unwrap();

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

	let queue = Queue::new(Pow2Size::P6, Pow2Size::P6).unwrap();
	let read = |h: &rt::Object, b| queue.submit_read(h.as_raw(), b).unwrap();
	let mut writes = Vec::<io_queue_rt::Write<_>>::new();

	let mut poll_out = read(&out, Vec::with_capacity(16));
	let mut poll_window = read(&window, Vec::with_capacity(128));

	let mut parser = Parser { state: ParserState::Idle };
	let mut next_draw = rt::time::Monotonic::MAX;
	loop {
		queue.poll();
		queue.wait(next_draw.duration_since(rt::time::Monotonic::now()));
		queue.process();

		// Finish writes
		for i in (0..writes.len()).rev() {
			if let Some((res, _)) = driver_utils::task::poll(&mut writes[i]) {
				writes.swap_remove(i);
				res.unwrap();
			}
		}

		// Window events
		if let Some((res, b)) = driver_utils::task::poll(&mut poll_window) {
			res.unwrap();
			match ipc_wm::Event::decode((&*b).try_into().unwrap()) {
				Ok(ipc_wm::Event::Resize(r)) => {
					drop_fb(&mut fb, width, height);
					width = r.x;
					height = r.y;
					fb = new_fb(width, height);
					next_draw = rt::time::Monotonic::ZERO;
				}
				Ok(ipc_wm::Event::Input(k)) if k.is_press() => {
					if let input::Type::Unicode(c) = k.ty {
						let mut b = [0; 4];
						let b = c.encode_utf8(&mut b).as_bytes();
						let b = Vec::from(b);
						let wr = queue.submit_write(inp.as_raw(), b).unwrap();
						writes.push(wr);
					}
				}
				Ok(ipc_wm::Event::Input(_)) => {}
				Err(e) => todo!("{:?}", e),
			}
			poll_window = read(&window, b);
		}

		// Process wrote something
		if let Some((res, b)) = driver_utils::task::poll(&mut poll_out) {
			let len = res.unwrap();
			for &c in &b[..len] {
				match parser.add(c) {
					None => continue,
					Some(Action::PushChar(c)) => rasterizer.push_char(c),
					Some(Action::PopChar) => rasterizer.pop_char(),
					Some(Action::NewLine) => rasterizer.new_line(),
					Some(Action::ClearLine) => rasterizer.clear_line(),
				}
			}
			let next_draw_t = rt::time::Monotonic::now()
				.checked_add(Duration::from_millis(33))
				.unwrap_or(rt::time::Monotonic::ZERO);
			next_draw = next_draw.min(next_draw_t);
			poll_out = read(&out, b);
		}

		// Draw window
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
