#![no_std]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{
	cell::{Cell, RefCell},
	future::Future,
	task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};
use nora_io_queue_rt::{Pow2Size, Queue};
use norostb_kernel::{error::Error, io};
use norostb_rt as rt;

fn main() {
	let mut args = rt::args::Args::new().skip(1);
	let table = args.next().expect("expected table path");
	let input = args.next().expect("expected input object path");

	// Create I/O queue with two entries: one for a job and one for reading
	let io_queue = Queue::new(Pow2Size::P1, Pow2Size::P1).unwrap();

	// Create a table
	let root = rt::io::file_root().unwrap();
	let table = rt::Object::create(&root, table).unwrap().into_raw();

	// Open input
	let input = rt::Object::open(&root, input).unwrap().into_raw();

	let char_buf = RefCell::new(VecDeque::new());
	let readers = RefCell::new(driver_utils::Arena::new());
	let pending_read = Cell::new(None);
	let shifts = Cell::new(0);

	let do_read = || async {
		use scancodes::{Event, ScanCode};
		let mut buf = io_queue
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
				let job = io::Job {
					ty: io::Job::READ,
					job_id,
					..Default::default()
				};
				buf.extend_from_slice(job.as_ref());
				buf.push(chr);
				io_queue.submit_write(table, buf).unwrap().await.unwrap();
				return true;
			} else {
				char_buf.borrow_mut().push_back(chr);
			}
		}
		false
	};

	let do_job = || async {
		let mut data = io_queue
			.submit_read(table, Vec::new(), 512)
			.unwrap()
			.await
			.unwrap();
		let (mut job, d) = io::Job::deserialize(&data).unwrap();
		assert_eq!(job.result, 0);
		match job.ty {
			io::Job::OPEN => {
				if job.handle == rt::Handle::MAX && d == b"stream" {
					job.handle = readers.borrow_mut().insert(());
				} else {
					job.result = Error::InvalidObject as i16;
				}
				data.clear();
				data.extend_from_slice(job.as_ref());
			}
			io::Job::CLOSE => {
				readers.borrow_mut().remove(job.handle).unwrap();
				// The kernel does not expect a response
				return true;
			}
			io::Job::READ => {
				// Ensure the handle is valid.
				readers.borrow_mut()[job.handle];
				data.clear();
				data.extend_from_slice(job.as_ref());
				let mut l = 0;
				for _ in 0..data.spare_capacity_mut().len() {
					if let Some(r) = char_buf.borrow_mut().pop_front() {
						data.push(r);
						l += 1;
					} else {
						break;
					}
				}
				if l == 0 {
					// There is currently no data, so delay a response until there is
					// data. Don't enqueue a new TAKE_JOB either so we have a slot
					// free for when we can send a reply
					pending_read.set(Some(job.job_id));
					return false;
				}
			}
			_ => {
				job.result = Error::InvalidOperation as i16;
				data.clear();
				data.extend_from_slice(job.as_ref());
			}
		};
		io_queue.submit_write(table, data).unwrap().await.unwrap();
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
		io_queue.poll();
		io_queue.wait();
		io_queue.process();
	}
}
