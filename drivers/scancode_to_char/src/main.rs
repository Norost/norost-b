#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{
	cell::{Cell, RefCell},
	future::Future,
	task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
	time::Duration,
};
use driver_utils::os::stream_table::{Request, Response, StreamTable};
use nora_io_queue_rt::Queue;
use norostb_kernel::{error::Error, object::Pow2Size, RWX};

#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{:?}", info));
	rt::exit(128)
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let mut args = rt::args::Args::new().skip(1);
	let table_name = args.next().expect("expected table path");
	let input = args.next().expect("expected input object path");

	let root = rt::io::file_root().unwrap();

	// Create a stream table
	let table = {
		let buf = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
		StreamTable::new(&buf, Pow2Size(5))
	};
	root.create(table_name)
		.unwrap()
		.share(&table.public_table())
		.unwrap();

	// Open input
	let input = rt::Object::open(&root, input).unwrap().into_raw();

	let char_buf = RefCell::new(VecDeque::new());
	let readers = RefCell::new(driver_utils::Arena::new());
	let pending_read = Cell::new(None);
	let shifts = Cell::new(0);

	// Create async I/O queue
	let queue = Queue::new(
		nora_io_queue_rt::Pow2Size::P6,
		nora_io_queue_rt::Pow2Size::P6,
	)
	.unwrap();

	let do_read = || async {
		use scancodes::{Event, ScanCode};
		let mut buf = queue
			.submit_read(input, Vec::new(), 4)
			.unwrap()
			.await
			.unwrap();
		assert_eq!(buf.len(), 4, "incomplete scancode");
		let chr = match Event::try_from(<[u8; 4]>::try_from(&buf[..]).unwrap()).unwrap() {
			Event::Press(ScanCode::LeftShift) | Event::Press(ScanCode::RightShift) => {
				shifts.set(shifts.get() + 1);
				None
			}
			Event::Release(ScanCode::LeftShift) | Event::Release(ScanCode::RightShift) => {
				shifts.set(shifts.get() - 1);
				None
			}
			Event::Press(s) => match s {
				ScanCode::Backspace => Some(0x7f), // DEL
				ScanCode::Enter => Some(b'\n'),
				ScanCode::ForwardSlash => Some(b'/'),
				ScanCode::BackSlash => Some(b'\\'),
				ScanCode::Colon => Some(b':'),
				ScanCode::Semicolon => Some(b';'),
				ScanCode::Comma => Some(b','),
				ScanCode::Dot => Some(b'.'),
				ScanCode::SingleQuote => Some(b'\''),
				ScanCode::DoubleQuote => Some(b'"'),
				ScanCode::Space => Some(b' '),
				ScanCode::Minus if shifts.get() == 0 => Some(b'-'),
				ScanCode::Minus if shifts.get() > 0 => Some(b'_'),
				s => s
					.alphabet_to_char()
					.or_else(|| s.bracket_to_char())
					.or_else(|| s.number_to_char())
					.map(|c| {
						if shifts.get() > 0 {
							c.to_ascii_uppercase() as u8
						} else {
							c as u8
						}
					}),
			},
			Event::Release(_) => None,
		};
		if let Some(chr) = chr {
			if let Some(job_id) = pending_read.take() {
				buf.clear();
				let data = table.alloc(1).expect("out of buffers");
				data.copy_from(0, &[chr]);
				table.enqueue(job_id, Response::Data { data, length: 1 });
				table.flush();
				return true;
			} else {
				char_buf.borrow_mut().push_back(chr);
			}
		}
		false
	};

	let do_job = || async {
		queue
			.submit_read(table.notifier().as_raw(), Vec::new(), 0)
			.unwrap()
			.await
			.unwrap();
		let mut tiny_buf = [0; 16];
		let (handle, req) = table.dequeue().unwrap();
		let (job_id, resp) = match req {
			Request::Open { job_id, path } => {
				let l = tiny_buf.len();
				let p = &mut tiny_buf[..l.min(path.len())];
				path.copy_to(0, p);
				path.manual_drop();
				if handle == rt::Handle::MAX && p == b"stream" {
					(job_id, Response::Handle(readers.borrow_mut().insert(())))
				} else {
					(job_id, Response::Error(Error::InvalidObject))
				}
			}
			Request::Close => {
				readers.borrow_mut().remove(handle).unwrap();
				// The kernel does not expect a response
				return true;
			}
			Request::Read {
				job_id,
				amount,
				peek,
			} => {
				if peek {
					(job_id, Response::Error(Error::InvalidOperation))
				} else {
					let mut char_buf = char_buf.borrow_mut();
					if char_buf.is_empty() {
						// There is currently no data, so delay a response until there is
						// data. Don't enqueue a new TAKE_JOB either so we have a slot
						// free for when we can send a reply
						pending_read.set(Some(job_id));
						return false;
					} else {
						let l = tiny_buf.len();
						let mut b = &mut tiny_buf[..l.min(char_buf.len())];
						for w in b.iter_mut() {
							*w = char_buf.pop_front().unwrap();
						}
						let data = table.alloc(b.len()).expect("out of buffers");
						data.copy_from(0, b);
						let length = l.try_into().unwrap();
						(job_id, Response::Data { data, length })
					}
				}
			}
			_ => todo!(),
		};
		table.enqueue(job_id, resp);
		table.flush();
		true
	};

	unsafe fn clone(p: *const ()) -> RawWaker {
		RawWaker::new(p, &VTBL)
	}
	unsafe fn wake(p: *const ()) {
		(&*(p as *const Cell<bool>)).set(true);
	}
	static VTBL: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, |_| ());

	macro_rules! w {
		($n:ident, $c:ident, $t:ident = $f:ident) => {
			let $n = Cell::new(true);
			let $c = RawWaker::new(&$n as *const _ as _, &VTBL);
			let $c = unsafe { Waker::from_raw($c) };
			let mut $c = Context::from_waker(&$c);
			let mut $t = Box::pin($f());
		};
	}

	w!(read_notif, read_cx, read_task = do_read);
	w!(job_notif, job_cx, job_task = do_job);

	loop {
		while read_notif.get() || job_notif.get() {
			if read_notif.take() {
				if let Poll::Ready(new_job) = read_task.as_mut().poll(&mut read_cx) {
					read_task = Box::pin(do_read());
					read_notif.set(true);
					if new_job {
						job_notif.set(true);
						job_task = Box::pin(do_job());
					}
				}
			}
			if job_notif.take() {
				if let Poll::Ready(new_job) = job_task.as_mut().poll(&mut job_cx) {
					if new_job {
						job_notif.set(true);
						job_task = Box::pin(do_job());
					}
				}
			}
		}
		queue.poll();
		queue.wait(Duration::MAX);
		queue.process();
	}
}
