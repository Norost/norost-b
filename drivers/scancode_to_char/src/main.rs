#![no_std]
#![feature(alloc_error_handler)]
#![feature(start)]

extern crate alloc;

use {
	alloc::{boxed::Box, collections::VecDeque, vec::Vec},
	async_std::{io::Read, object::RefAsyncObject},
	core::{
		cell::{Cell, RefCell},
		future::Future,
		task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
		time::Duration,
	},
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	norostb_kernel::{error::Error, object::Pow2Size},
	rt_default as _,
};

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let mut args = rt::args::args().skip(1);
	let table_name = args.next().expect("expected table path");
	let input = args.next().expect("expected input object path");

	let root = rt::io::file_root().unwrap();

	// Create a stream table
	let table = {
		let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
		StreamTable::new(&buf, Pow2Size(5), (1 << 5) - 1)
	};
	root.create(table_name)
		.unwrap()
		.share(table.public())
		.unwrap();

	// Open input
	let input = rt::Object::open(&root, input).unwrap();
	let input = async_std::object::AsyncObject::from(input);

	let char_buf = RefCell::new(VecDeque::new());
	let readers = RefCell::new(driver_utils::Arena::new());
	let pending_read = Cell::new(None);

	let do_read = || async {
		use input::{Input, Type};
		// FIXME https://github.com/rust-lang/rust/issues/99385
		// It *was* fine up until recently. Imma keep using it for now...
		let (res, mut buf) = input.read(Vec::with_capacity(8)).await;
		res.unwrap();
		assert_eq!(buf.len(), 8, "incomplete input");
		let evt = u64::from_le_bytes(buf[..].try_into().unwrap());
		let chr = Input::try_from(evt)
			.ok()
			.and_then(|evt| match (evt.is_press(), evt.ty) {
				(true, Type::Unicode(c)) => Some(c),
				_ => None,
			});
		if let Some(chr) = chr {
			let mut b = [0; 4];
			let chr = chr.encode_utf8(&mut b).as_bytes();
			if let Some(job_id) = pending_read.take() {
				buf.clear();
				let data = table.alloc(chr.len()).expect("out of buffers");
				data.copy_from(0, chr);
				table.enqueue(job_id, Response::Data(data));
				table.flush();
				return true;
			} else {
				char_buf.borrow_mut().extend(chr);
			}
		}
		false
	};

	let do_job = || async {
		let (res, _buf) = RefAsyncObject::from(table.notifier()).read(()).await;
		res.unwrap();
		let mut tiny_buf = [0; 16];
		let (handle, job_id, req) = table.dequeue().unwrap();
		let (job_id, resp) = match req {
			Request::Open { path } => {
				let l = tiny_buf.len();
				let p = &mut tiny_buf[..l.min(path.len())];
				path.copy_to(0, p);
				if handle == rt::Handle::MAX && p == b"stream" {
					(job_id, Response::Handle(readers.borrow_mut().insert(())))
				} else {
					(job_id, Response::Error(Error::InvalidObject))
				}
			}
			Request::Close => {
				if handle != rt::Handle::MAX {
					// TODO we should have some kind of global refcount to see if we
					// should exit
					readers.borrow_mut().remove(handle).unwrap();
				}
				// The kernel does not expect a response
				return true;
			}
			Request::Read { amount: _ } => {
				let mut char_buf = char_buf.borrow_mut();
				if char_buf.is_empty() {
					// There is currently no data, so delay a response until there is
					// data. Don't enqueue a new TAKE_JOB either so we have a slot
					// free for when we can send a reply
					pending_read.set(Some(job_id));
					return false;
				} else {
					let l = tiny_buf.len();
					let b = &mut tiny_buf[..l.min(char_buf.len())];
					for w in b.iter_mut() {
						*w = char_buf.pop_front().unwrap();
					}
					let data = table.alloc(b.len()).expect("out of buffers");
					data.copy_from(0, b);
					(job_id, Response::Data(data))
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
		async_std::queue::wait(Duration::MAX);
	}
}
