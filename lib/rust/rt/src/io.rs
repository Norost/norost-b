#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use crate::{tls, RefObject};
use alloc::{boxed::Box, vec::Vec};
use core::{
	future::Future,
	mem,
	pin::Pin,
	ptr::NonNull,
	sync::atomic::Ordering,
	task::{Context, Poll as PollF, RawWaker, RawWakerVTable, Waker},
};
pub use nora_io_queue_rt::{
	error::Result, Create, Handle, Open, Peek, Poll, Pow2Size, Queue, Read, Seek, SeekFrom, Share,
	Write,
};
use norostb_kernel::syscall;

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
		Box::from_raw(ptr.cast::<Queue>());
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
	// 2^6 = 64, 32 * 64 + 16 * 64 = 3072, which fits in a single page.
	Queue::new(Pow2Size::P6, Pow2Size::P6)
		.map(|q| [(QUEUE_KEY, Box::into_raw(q.into()).cast())].into_iter())
}

fn queue() -> &'static Queue {
	// SAFETY: only safe if the queue has already been initialized, which it should be.
	unsafe { &*crate::tls::get(QUEUE_KEY).cast::<Queue>() }
}

#[inline]
pub fn read(handle: Handle, buf: Vec<u8>, amount: usize) -> Read<'static> {
	queue()
		.submit_read(handle, buf, amount)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn peek(handle: Handle, buf: Vec<u8>, amount: usize) -> Peek<'static> {
	queue()
		.submit_peek(handle, buf, amount)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn write(handle: Handle, data: Vec<u8>) -> Write<'static> {
	queue()
		.submit_write(handle, data)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn open(handle: Handle, path: Vec<u8>) -> Open<'static> {
	queue()
		.submit_open(handle, path)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn create(handle: Handle, path: Vec<u8>) -> Create<'static> {
	queue()
		.submit_create(handle, path)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn seek(handle: Handle, from: SeekFrom) -> Seek<'static> {
	queue()
		.submit_seek(handle, from)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn poll(handle: Handle) -> Poll<'static> {
	queue().submit_poll(handle).unwrap_or_else(|_| todo!())
}

#[inline]
pub fn close(handle: Handle) {
	queue().submit_close(handle).unwrap_or_else(|_| todo!())
}

#[inline]
pub fn share(handle: Handle, share: Handle) -> Share<'static> {
	queue()
		.submit_share(handle, share)
		.unwrap_or_else(|_| todo!())
}

#[inline]
pub fn duplicate(handle: Handle) -> Result<Handle> {
	syscall::duplicate_handle(handle).map_err(|_| todo!())
}

#[inline]
pub fn create_root() -> Result<Handle> {
	syscall::create_root().map_err(|_| todo!())
}

/// Block on an asynchronous I/O task until it is finished.
pub fn block_on<T, R>(fut: T) -> R
where
	T: Future<Output = R>,
{
	static DUMMY: RawWakerVTable =
		RawWakerVTable::new(|_| RawWaker::new(0 as _, &DUMMY), |_| (), |_| (), |_| ());

	let waker = unsafe { Waker::from_raw(RawWaker::new(0 as _, &DUMMY)) };
	let mut cx = Context::from_waker(&waker);

	// We don't use pin_utils because it doesn't have rustc-dep-of-std
	let mut fut = fut;
	// SAFETY: we shadow the original Future and hence can't move it.
	let mut fut = unsafe { Pin::new_unchecked(&mut fut) };
	if let PollF::Ready(res) = Pin::new(&mut fut).poll(&mut cx) {
		return res;
	}
	let queue = queue();
	loop {
		queue.poll();
		queue.wait();
		queue.process();
		if let PollF::Ready(res) = Pin::new(&mut fut).poll(&mut cx) {
			return res;
		}
	}
}
