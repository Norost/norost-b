//! # Async I/O queue with runtime.

#![no_std]
#![deny(unused)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

pub use nora_io_queue::{error, Handle, Monotonic, Pow2Size, SeekFrom};

use alloc::boxed::Box;
use arena::Arena;
use async_completion::{Buf, BufMut};
use core::{
	cell::{Cell, RefCell},
	fmt,
	future::Future,
	mem::{self, MaybeUninit},
	pin::Pin,
	slice,
	task::{Context, Poll, Waker},
	time::Duration,
};
use nora_io_queue::{self as q, Request};

pub struct Queue {
	inner: RefCell<q::Queue>,
	inflight_buffers: RefCell<Arena<BufferFutureState, ()>>,
	/// A counter of responses that have been popped of but have not yet been consumed
	/// by the client.
	///
	/// This is used to avoid a race condition with [`Queue::wait`], where a request may
	/// not have finished yet at the moment of a poll but intermediate I/O requests between
	/// may cause the response for this request to be popped off before `wait()`. To avoid this,
	/// wait will return immediately if this counter is nonzero.
	ready_responses: Cell<usize>,
}

impl fmt::Debug for Queue {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(stringify!(Queue))
			.field("inner", &self.inner)
			// Don't print the potentially huge inflight_buffers list.
			.finish_non_exhaustive()
	}
}

impl Queue {
	pub fn new(requests_size: Pow2Size, responses_size: Pow2Size) -> error::Result<Self> {
		q::Queue::new(requests_size, responses_size).map(|inner| Self {
			inner: inner.into(),
			inflight_buffers: Arena::new().into(),
			ready_responses: 0.into(),
		})
	}

	pub fn requests_size(&self) -> Pow2Size {
		self.inner.borrow().requests_size()
	}

	pub fn responses_size(&self) -> Pow2Size {
		self.inner.borrow().responses_size()
	}

	/// Submit a request involving reading into byte buffers.
	fn submit_read_buffer<B: BufMut, F>(
		&self,
		mut buffer: B,
		handle: Handle,
		wrap: F,
	) -> Result<BufferFuture<'_, B>, Full<B>>
	where
		F: FnOnce(&'static mut [MaybeUninit<u8>]) -> Request,
	{
		let mut inflight = self.inflight_buffers.borrow_mut();
		let i = inflight.insert(BufferFutureState::Inflight);
		// SAFETY: The buffer will live at least as long as the BufferFuture,
		// even if it is mem::forgot()ten
		let buf = unsafe { extend_lifetime_mut(buf_as_slice_mut(&mut buffer)) };
		let res = self
			.inner
			.borrow_mut()
			.submit(i.into_raw().0 as u64, handle, wrap(buf));
		match res {
			Ok(_) => Ok(BufferFuture {
				queue: self,
				inflight_index: i,
				buffer: Some(buffer),
			}),
			Err(_) => {
				inflight.remove(i);
				Err(Full(buffer))
			}
		}
	}

	/// Submit a request involving writing from byte buffers.
	///
	/// Only data from `buf[offset..]` is written from start to end. Not all data may be written.
	///
	/// # Panics
	///
	/// If `offset` is larger than the length of `buf`.
	fn submit_write_buffer<B: Buf, F>(
		&self,
		buffer: B,
		handle: Handle,
		wrap: F,
	) -> Result<BufferFuture<'_, B>, Full<B>>
	where
		F: FnOnce(&'static [u8]) -> Request,
	{
		let mut inflight = self.inflight_buffers.borrow_mut();
		let i = inflight.insert(BufferFutureState::Inflight);
		// SAFETY: The buffer will live at least as long as the BufferFuture,
		// even if it is mem::forgot()ten
		let buf = unsafe { extend_lifetime(buf_as_slice(&buffer)) };
		let res = self
			.inner
			.borrow_mut()
			.submit(i.into_raw().0 as u64, handle, wrap(buf));
		match res {
			Ok(_) => Ok(BufferFuture {
				queue: self,
				inflight_index: i,
				buffer: Some(buffer),
			}),
			Err(_) => {
				inflight.remove(i);
				Err(Full(buffer))
			}
		}
	}

	/// Submit a request not involving a byte buffer.
	///
	/// While a `BufferFuture` is returned the buffer is a dummy.
	fn submit_no_buffer(
		&self,
		handle: Handle,
		request: Request,
	) -> Result<BufferFuture<'_, ()>, Full<()>> {
		self.submit_write_buffer((), handle, |_| request)
	}

	/// Read data from an object, advancing the seek head.
	pub fn submit_read<B>(&self, handle: Handle, buf: B) -> Result<Read<'_, B>, Full<B>>
	where
		B: BufMut,
	{
		self.submit_read_buffer(buf, handle, |buffer| Request::Read { buffer })
			.map(|fut| Read { fut })
	}

	/// Read data from an object without advancing the seek head.
	pub fn submit_peek<B>(&self, handle: Handle, buf: B) -> Result<Peek<'_, B>, Full<B>>
	where
		B: BufMut,
	{
		self.submit_read_buffer(buf, handle, |buffer| Request::Peek { buffer })
			.map(|fut| Peek { fut })
	}

	/// Write data to an object.
	pub fn submit_write<B>(&self, handle: Handle, data: B) -> Result<Write<'_, B>, Full<B>>
	where
		B: Buf,
	{
		self.submit_write_buffer(data, handle, |buffer| Request::Write { buffer })
			.map(|fut| Write { fut })
	}

	/// Open an object.
	pub fn submit_open<B>(&self, handle: Handle, path: B) -> Result<Open<'_, B>, Full<B>>
	where
		B: Buf,
	{
		self.submit_write_buffer(path, handle, |path| Request::Open { path })
			.map(|fut| Open { fut })
	}

	/// Create an object.
	pub fn submit_create<B>(&self, handle: Handle, path: B) -> Result<Create<'_, B>, Full<B>>
	where
		B: Buf,
	{
		self.submit_write_buffer(path, handle, |path| Request::Create { path })
			.map(|fut| Create { fut })
	}

	pub fn submit_seek(&self, handle: Handle, from: SeekFrom) -> Result<Seek<'_>, Full<()>> {
		self.submit_no_buffer(handle, Request::Seek { from })
			.map(|fut| Seek { fut })
	}

	pub fn submit_close(&self, handle: Handle) -> Result<(), Full<()>> {
		self.inner
			.borrow_mut()
			.submit(u64::MAX, handle, Request::Close)
			.map(|b| debug_assert!(!b))
			.map_err(|_| Full(()))
	}

	pub fn submit_share(&self, handle: Handle, share: Handle) -> Result<Share<'_>, Full<()>> {
		self.submit_no_buffer(handle, Request::Share { share })
			.map(|fut| Share { fut })
	}

	pub fn process(&self) {
		let mut inner = self.inner.borrow_mut();
		let mut inflight = self.inflight_buffers.borrow_mut();
		let mut n = 0;
		while let Some(resp) = inner.receive() {
			n += 1;
			let i = arena::Handle::from_raw(resp.user_data as usize, ());
			let s = BufferFutureState::Finished(error::result(resp.value).map(|v| v as u64));
			match mem::replace(&mut inflight[i], s) {
				BufferFutureState::Cancelled(_) => {
					inflight.remove(i).unwrap();
					n -= 1;
				}
				BufferFutureState::InflightWithWaker(w) => w.wake(),
				_ => {}
			}
		}
		self.ready_responses.set(self.ready_responses.get() + n);
	}

	pub fn poll(&self) -> Monotonic {
		self.inner.borrow_mut().poll()
	}

	pub fn wait(&self, timeout: Duration) -> Option<Monotonic> {
		(self.ready_responses.get() == 0).then(|| self.inner.borrow_mut().wait(timeout))
	}
}

/// # Safety
///
/// The object must exist for at least as long as the static lifetime reference is used.
unsafe fn extend_lifetime<'a, T: ?Sized>(t: &'a T) -> &'static T {
	unsafe { mem::transmute(t) }
}

/// # Safety
///
/// The object must exist for at least as long as the static lifetime reference is used.
unsafe fn extend_lifetime_mut<'a, T: ?Sized>(t: &'a mut T) -> &'static mut T {
	unsafe { mem::transmute(t) }
}

fn buf_as_slice<B: Buf>(buf: &B) -> &[u8] {
	// SAFETY: the Buf impl guarantees the returned pointer and length are valid.
	unsafe { slice::from_raw_parts(buf.as_ptr().cast(), buf.bytes_total()) }
}

fn buf_as_slice_mut<B: BufMut>(buf: &mut B) -> &mut [MaybeUninit<u8>] {
	// SAFETY: the Buf impl guarantees the returned pointer and length are valid.
	unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr().cast(), buf.bytes_total()) }
}

/// Structure returned if the queue is full.
/// It contains the buffer that was passed as argument.
pub struct Full<B: Buf>(pub B);

/// Custom debug impl since there is no need to print the inner buffer.
impl<B: Buf> fmt::Debug for Full<B> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		"Full".fmt(f)
	}
}

enum BufferFutureState {
	Inflight,
	InflightWithWaker(Waker),
	Finished(error::Result<u64>),
	Cancelled(Box<dyn Buf>),
}

/// A future that involves byte buffers.
struct BufferFuture<'a, B: Buf> {
	queue: &'a Queue,
	inflight_index: arena::Handle<()>,
	buffer: Option<B>,
}

impl<B: Buf> Future for BufferFuture<'_, B> {
	type Output = (Result<u64, error::Error>, B);

	/// Check if the read request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		let i = self.inflight_index;
		let mut inflight = self.queue.inflight_buffers.borrow_mut();
		let t = &mut inflight[i];
		match mem::replace(t, BufferFutureState::Cancelled(Box::new(()))) {
			BufferFutureState::Inflight => {
				*t = BufferFutureState::InflightWithWaker(cx.waker().clone());
				Poll::Pending
			}
			BufferFutureState::InflightWithWaker(waker) => {
				*t = BufferFutureState::InflightWithWaker(if waker.will_wake(cx.waker()) {
					waker
				} else {
					cx.waker().clone()
				});
				Poll::Pending
			}
			BufferFutureState::Finished(res) => {
				inflight.remove(i).unwrap();
				self.queue
					.ready_responses
					.set(self.queue.ready_responses.get() - 1);
				Poll::Ready((res, self.buffer.take().expect("buffer already taken")))
			}
			BufferFutureState::Cancelled(_) => unreachable!(),
		}
	}
}

impl<B: Buf> Drop for BufferFuture<'_, B> {
	fn drop(&mut self) {
		if let Some(buf) = self.buffer.take() {
			let i = self.inflight_index;
			let mut inflight = self.queue.inflight_buffers.borrow_mut();
			match inflight.get_mut(i) {
				Some(s @ BufferFutureState::Inflight)
				| Some(s @ BufferFutureState::InflightWithWaker(_)) => {
					// We can't drop the buffer yet as it is still in use by the queue.
					*s = BufferFutureState::Cancelled(Box::new(buf));
				}
				Some(BufferFutureState::Finished(_)) | None => {}
				Some(BufferFutureState::Cancelled(_)) => unreachable!(),
			}
		}
	}
}

fn poll_set_len<B: BufMut>(
	fut: &mut BufferFuture<'_, B>,
	cx: &mut Context<'_>,
) -> Poll<(error::Result<usize>, B)> {
	Pin::new(fut).poll(cx).map(|(r, mut buf)| {
		let r = r.map(|s| {
			// SAFETY: the kernel should have initialized the exact given amount of bytes.
			unsafe { buf.set_bytes_init(s as _) };
			s as _
		});
		(r, buf)
	})
}

/// A pending read request.
pub struct Read<'a, B: BufMut> {
	fut: BufferFuture<'a, B>,
}

impl<B: BufMut> Future for Read<'_, B> {
	type Output = (error::Result<usize>, B);

	/// Check if the read request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		poll_set_len(&mut self.fut, cx)
	}
}

/// A pending peek request.
pub struct Peek<'a, B: BufMut> {
	fut: BufferFuture<'a, B>,
}

impl<B: BufMut> Future for Peek<'_, B> {
	type Output = (error::Result<usize>, B);

	/// Check if the peek request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		poll_set_len(&mut self.fut, cx)
	}
}

/// A pending write request.
pub struct Write<'a, B: Buf> {
	fut: BufferFuture<'a, B>,
}

impl<B: Buf> Future for Write<'_, B> {
	type Output = (error::Result<usize>, B);

	/// Check if the write request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		Pin::new(&mut self.fut)
			.poll(cx)
			.map(|(r, buf)| (r.map(|s| s as _), buf))
	}
}

/// A pending open request.
pub struct Open<'a, B: Buf> {
	fut: BufferFuture<'a, B>,
}

impl<B: Buf> Future for Open<'_, B> {
	type Output = (error::Result<Handle>, B);

	/// Check if the open request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		Pin::new(&mut self.fut)
			.poll(cx)
			.map(|(r, buf)| (r.map(|s| s as _), buf))
	}
}

/// A pending create request.
pub struct Create<'a, B: Buf> {
	fut: BufferFuture<'a, B>,
}

impl<B: Buf> Future for Create<'_, B> {
	type Output = (error::Result<Handle>, B);

	/// Check if the create request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		Pin::new(&mut self.fut)
			.poll(cx)
			.map(|(r, buf)| (r.map(|s| s as _), buf))
	}
}

/// A pending seek request.
pub struct Seek<'a> {
	fut: BufferFuture<'a, ()>,
}

impl Future for Seek<'_> {
	type Output = Result<u64, error::Error>;

	/// Check if the seek request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(r, _)| r)
	}
}

/// A pending share request.
pub struct Share<'a> {
	fut: BufferFuture<'a, ()>,
}

impl Future for Share<'_> {
	type Output = Result<u64, error::Error>;

	/// Check if the share request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(r, _)| r)
	}
}
