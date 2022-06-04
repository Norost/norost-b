#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use crate::{tls, RefObject};
use alloc::boxed::Box;
use core::{
	mem::{self, MaybeUninit},
	ptr::NonNull,
	sync::atomic::Ordering,
};
use norostb_kernel::{error::result, io::Queue, syscall};
use norostb_kernel::{error::Error, Handle};

pub use norostb_kernel::{
	error::Result,
	io::{Job, Request, Response, SeekFrom},
};

macro_rules! transmute_handle {
	($fn:ident, $set_fn:ident -> $handle:ident) => {
		#[inline(always)]
		pub fn $fn() -> Option<RefObject<'static>> {
			let h = crate::globals::GLOBALS
				.get_ref()
				.$handle
				.load(Ordering::Relaxed);
			(h != Handle::MAX).then(|| RefObject::from_raw(h))
		}

		#[inline(always)]
		pub fn $set_fn(h: Option<RefObject<'static>>) {
			let h = h.map_or(Handle::MAX, |h| h.into_raw());
			// SAFETY: $handle is only set once at the start of the program
			crate::globals::GLOBALS
				.get_ref()
				.$handle
				.store(h, Ordering::Relaxed);
		}
	};
}

transmute_handle!(stdin, set_stdin -> stdin_handle);
transmute_handle!(stdout, set_stdout -> stdout_handle);
transmute_handle!(stderr, set_stderr -> stderr_handle);
transmute_handle!(file_root, set_file_root -> file_root_handle);
transmute_handle!(net_root, set_net_root -> net_root_handle);
transmute_handle!(process_root, set_process_root -> process_root_handle);

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
		let queue = Box::from_raw(ptr.cast::<Queue>());
		syscall::destroy_io_queue(queue.base.cast()).unwrap_or_else(|_| core::intrinsics::abort());
	}
}

/// Initialize the runtime.
///
/// # Safety
///
/// This function may only be called once.
///
/// TLS storage must be initialized with [`crate::tls::init`].
pub(crate) unsafe fn init(_arguments: Option<NonNull<u8>>) {
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
pub fn open(handle: Handle, path: &[u8]) -> Result<Handle> {
	result(enqueue(Request::open(0, handle, path)).value).map(|v| v as Handle)
}

/// Blocking create
#[inline]
pub fn create(handle: Handle, path: &[u8]) -> Result<Handle> {
	result(enqueue(Request::create(0, handle, path)).value).map(|v| v as Handle)
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

/// Blocking share
#[inline]
pub fn share(handle: Handle, share: Handle) -> Result<u64> {
	result(enqueue(Request::share(0, handle, share)).value).map(|v| v as u64)
}

/// Blocking duplicate
#[inline]
pub fn duplicate(handle: Handle) -> Result<Handle> {
	syscall::duplicate_handle(handle).map_err(|_| todo!())
}

/// Blocking create root
#[inline]
pub fn create_root() -> Result<Handle> {
	syscall::create_root().map_err(|_| todo!())
}
