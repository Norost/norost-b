#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use crate::tls;
use alloc::boxed::Box;
use core::mem::{self, MaybeUninit};
use norostb_kernel::{error::result, io::Queue, syscall};
use norostb_kernel::{error::Error, Handle};

pub use norostb_kernel::{
	error::Result,
	io::{Job, ObjectInfo, Request, Response, SeekFrom},
};

#[derive(Copy, Clone)]
pub struct IoSlice<'a>(&'a [u8]);

impl<'a> IoSlice<'a> {
	#[inline]
	pub fn new(buf: &'a [u8]) -> IoSlice<'a> {
		IoSlice(buf)
	}

	#[inline]
	pub fn advance(&mut self, n: usize) {
		self.0 = &self.0[n..]
	}

	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self.0
	}
}

pub struct IoSliceMut<'a>(&'a mut [u8]);

impl<'a> IoSliceMut<'a> {
	#[inline]
	pub fn new(buf: &'a mut [u8]) -> IoSliceMut<'a> {
		IoSliceMut(buf)
	}

	#[inline]
	pub fn advance(&mut self, n: usize) {
		let slice = mem::replace(&mut self.0, &mut []);
		let (_, remaining) = slice.split_at_mut(n);
		self.0 = remaining;
	}

	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self.0
	}

	#[inline]
	pub fn as_mut_slice(&mut self) -> &mut [u8] {
		self.0
	}
}

// Queue key allocation is hardcoded in tls.rs
const QUEUE_KEY: tls::Key = tls::Key(0);
pub(crate) unsafe extern "C" fn queue_dtor(ptr: *mut ()) {
	unsafe {
		let _queue = Box::from_raw(ptr.cast::<Queue>());
		// TODO dealloc the queue itself.
		todo!()
	}
}

/// Initialize the runtime.
///
/// # Safety
///
/// This function may only be called once.
///
/// TLS storage must be initialized with [`crate::tls::init`].
pub(crate) unsafe fn init() {
	let (k, v) = create_for_thread()
		.ok()
		.and_then(|mut it| it.next())
		.unwrap_or_else(|| {
			// Ditto
			core::intrinsics::abort()
		});
	unsafe {
		tls::set(k, v);
	}
}

/// Create & initialize I/O for a new thread.
#[must_use = "the values must be put in TLS storage"]
pub(crate) fn create_for_thread() -> Result<impl Iterator<Item = (tls::Key, *mut ())>> {
	syscall::create_io_queue(None, 0, 0)
		.map_err(|_| Error::Unknown)
		.map(|base| {
			[Box::new(Queue {
				base: base.cast(),
				requests_mask: 0,
				responses_mask: 0,
			})]
			.into_iter()
			.map(|b| (QUEUE_KEY, Box::into_raw(b).cast()))
		})
}

fn enqueue(request: Request) -> Response {
	unsafe {
		let queue = &mut *crate::tls::get(QUEUE_KEY).cast::<Queue>();
		queue.enqueue_request(request).unwrap();
		syscall::process_io_queue(Some(queue.base.cast())).unwrap();
		loop {
			if let Ok(e) = queue.dequeue_response() {
				break e;
			}
			syscall::wait_io_queue(Some(queue.base.cast())).unwrap();
		}
	}
}

/// Blocking read
#[inline]
pub fn read(handle: Handle, data: &mut [u8]) -> Result<usize> {
	result(enqueue(Request::read(0, handle, data)).value).map(|v| v as usize)
}

/// Blocking read
#[inline]
pub fn read_uninit(handle: Handle, data: &mut [MaybeUninit<u8>]) -> Result<usize> {
	result(enqueue(Request::read_uninit(0, handle, data)).value).map(|v| v as usize)
}

/// Blocking peek
#[inline]
pub fn peek(handle: Handle, data: &mut [u8]) -> Result<usize> {
	result(enqueue(Request::peek(0, handle, data)).value).map(|v| v as usize)
}

/// Blocking peek
#[inline]
pub fn peek_uninit(handle: Handle, data: &mut [MaybeUninit<u8>]) -> Result<usize> {
	result(enqueue(Request::peek_uninit(0, handle, data)).value).map(|v| v as usize)
}

/// Blocking write
#[inline]
pub fn write(handle: Handle, data: &[u8]) -> Result<usize> {
	result(enqueue(Request::write(0, handle, data)).value).map(|v| v as usize)
}

/// Blocking open
#[inline]
pub fn open(table: syscall::TableId, path: &[u8]) -> Result<Handle> {
	result(enqueue(Request::open(0, table, path)).value).map(|v| v as Handle)
}

/// Blocking create
#[inline]
pub fn create(table: syscall::TableId, path: &[u8]) -> Result<Handle> {
	result(enqueue(Request::create(0, table, path)).value).map(|v| v as Handle)
}

/// Blocking query
#[inline]
pub fn query(table: syscall::TableId, path: &[u8]) -> Result<Handle> {
	result(enqueue(Request::query(0, table, path)).value).map(|v| v as Handle)
}

/// Blocking query_next
#[inline]
pub fn query_next(query: Handle, info: &mut ObjectInfo) -> Result<bool> {
	let e = enqueue(Request::query_next(0, query, info));
	if e.value < 0 {
		// FIXME the API for quering is kinda broken right now.
		//Err(io::const_io_error!(io::ErrorKind::Uncategorized, "failed to advance query"))
		Ok(false)
	} else {
		Ok(e.value > 0)
	}
}

/// Blocking take_job
#[inline]
pub fn take_job(table: Handle, job: &mut Job) -> Result<()> {
	result(enqueue(Request::take_job(0, table, job)).value).map(|_| ())
}

/// Blocking finish_job
#[inline]
pub fn finish_job(table: Handle, job: &Job) -> Result<()> {
	result(enqueue(Request::finish_job(0, table, &job)).value).map(|_| ())
}

/// Blocking seek
#[inline]
pub fn seek(handle: Handle, from: SeekFrom) -> Result<u64> {
	result(enqueue(Request::seek(0, handle, from)).value).map(|v| v as u64)
}

/// Blocking poll
#[inline]
pub fn poll(handle: Handle) -> Result<usize> {
	result(enqueue(Request::poll(0, handle)).value).map(|v| v as usize)
}

/// Blocking close
#[inline]
pub fn close(handle: Handle) {
	enqueue(Request::close(0, handle));
}

/// Blocking duplicate
#[inline]
pub fn duplicate(handle: Handle) -> Result<Handle> {
	syscall::duplicate_handle(handle).map_err(|_| todo!())
}
