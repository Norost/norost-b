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
use nora_io_queue_rt::Full;
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

// Blocking has been chosen since the alternative requires storing requests somewhere
// else temporarily. This storage has to have an unbounded size and is likely a worse
// alternative than blocking and/or increasing the size of the queue.
macro_rules! impl_io {
	(None $fn:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret_ty:ident, $qfn:ident) => {
		#[doc = concat!(
			"An asynchronous ",
			stringify!($fn),
			" request. This may block if the queue is full.",
		)]
		pub fn $fn(handle: Handle $(,$arg: $arg_ty)*) -> $ret_ty<'static> {
			let q = queue();
			loop {
				match q.$qfn(handle, $($arg,)*) {
					Ok(r) => return r,
					Err(Full(_)) => {
						q.poll();
						q.wait();
						q.process();
					}
				}
			}
		}
	};
	($buf:ident $fn:ident($($arg:ident: $arg_ty:ty),* $(,)?) -> $ret_ty:ident, $qfn:ident) => {
		#[doc = concat!(
			"An asynchronous ",
			stringify!($fn),
			" request. This may block if the queue is full.",
		)]
		pub fn $fn(handle: Handle, mut $buf: Vec<u8> $(,$arg: $arg_ty)*) -> $ret_ty<'static> {
			let q = queue();
			loop {
				$buf = match q.$qfn(handle, $buf, $($arg,)*) {
					Ok(r) => return r,
					Err(Full(b)) => {
						q.poll();
						q.wait();
						q.process();
						b
					}
				}
			}
		}
	};
}

impl_io!(buf read(amount: usize) -> Read, submit_read);
impl_io!(buf peek(amount: usize) -> Peek, submit_peek);
impl_io!(data write(offset: usize) -> Write, submit_write);
impl_io!(path open(offset: usize) -> Open, submit_open);
impl_io!(path create(offset: usize) -> Create, submit_create);
impl_io!(None seek(from: SeekFrom) -> Seek, submit_seek);
impl_io!(None poll() -> Poll, submit_poll);
impl_io!(None share(share: Handle) -> Share, submit_share);

#[inline]
pub fn close(handle: Handle) {
	let q = queue();
	while q.submit_close(handle).is_err() {
		q.poll();
		q.wait();
		q.process();
	}
}

#[inline]
pub fn duplicate(handle: Handle) -> Result<Handle> {
	syscall::duplicate_handle(handle)
}

#[inline]
pub fn create_root() -> Result<Handle> {
	syscall::create_root()
}

/// Poll & process the I/O queue once.
pub fn poll_queue() {
	let q = queue();
	q.poll();
	q.process();
}

/// Poll & process the I/O queue once, waiting until any responses are available.
pub fn poll_queue_and_wait() {
	let q = queue();
	q.poll();
	q.wait();
	q.process();
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
	let q = queue();
	loop {
		q.poll();
		q.wait();
		q.process();
		if let PollF::Ready(res) = Pin::new(&mut fut).poll(&mut cx) {
			return res;
		}
	}
}
