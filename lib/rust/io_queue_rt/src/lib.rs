//! # Async I/O queue with runtime.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

pub use nora_io_queue::{error, Handle, Pow2Size, SeekFrom};

use alloc::vec::Vec;
use arena::Arena;
use core::{
	cell::{Cell, RefCell},
	fmt,
	future::Future,
	mem::{self, MaybeUninit},
	pin::Pin,
	task::{Context, Poll as TPoll, Waker},
};
use nora_io_queue::{self as q, Request};

pub struct Queue {
	inner: RefCell<q::Queue>,
	inflight_buffers: RefCell<Arena<(Vec<u8>, BufferFutureState), ()>>,
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
		})
	}

	/// Submit a request involving reading into byte buffers.
	fn submit_read_buffer<F>(
		&self,
		mut buf: Vec<u8>,
		handle: Handle,
		n: usize,
		wrap: F,
	) -> Result<BufferFuture<'_>, ()>
	where
		F: FnOnce(&'static mut [MaybeUninit<u8>]) -> Request,
	{
		buf.clear();
		buf.reserve(n);
		let mut inflight = self.inflight_buffers.borrow_mut();
		let i = inflight.insert((buf, BufferFutureState::Inflight));
		let buffer = &mut inflight[i].0.spare_capacity_mut()[..n];
		// SAFETY:
		// - The buffer will live at least as long as this queue due to us putting
		//   it in inflight_buffers. inflight_buffers can only be allocated through dropping.
		// - The destructor of the inner queue ensures no more requests are in flight.
		// - If this queue is mem::forgot()ten then the buffer lives forever.
		let buffer = unsafe { extend_lifetime_mut(buffer) };
		self.inner
			.borrow_mut()
			.submit(i.into_raw().0 as u64, handle, wrap(buffer))
			.unwrap_or_else(|e| todo!("{:?}", e));
		Ok(BufferFuture {
			queue: Some(self).into(),
			inflight_index: i,
		})
	}

	/// Submit a request involving writing from byte buffers.
	fn submit_write_buffer<F>(
		&self,
		buf: Vec<u8>,
		handle: Handle,
		wrap: F,
	) -> Result<BufferFuture<'_>, ()>
	where
		F: FnOnce(&'static [u8]) -> Request,
	{
		let mut inflight = self.inflight_buffers.borrow_mut();
		let i = inflight.insert((buf, BufferFutureState::Inflight));
		let buffer = &inflight[i].0[..];
		// SAFETY:
		// - The buffer will live at least as long as this queue due to us putting
		//   it in inflight_buffers. inflight_buffers can only be deallocated through dropping.
		// - The destructor of the inner queue ensures no more requests are in flight.
		// - If this queue is mem::forgot()ten then the buffer lives forever.
		let buffer = unsafe { extend_lifetime(buffer) };
		self.inner
			.borrow_mut()
			.submit(i.into_raw().0 as u64, handle, wrap(buffer))
			.unwrap_or_else(|e| todo!("{:?}", e));
		Ok(BufferFuture {
			queue: Some(self).into(),
			inflight_index: i,
		})
	}

	/// Submit a request not involving a byte buffer.
	///
	/// While a `BufferFuture` is returned the `Vec` is a dummy.
	fn submit_no_buffer(&self, handle: Handle, request: Request) -> Result<BufferFuture<'_>, ()> {
		let mut inflight = self.inflight_buffers.borrow_mut();
		let i = inflight.insert((Vec::new(), BufferFutureState::Inflight));
		self.inner
			.borrow_mut()
			.submit(i.into_raw().0 as u64, handle, request)
			.unwrap_or_else(|e| todo!("{:?}", e));
		Ok(BufferFuture {
			queue: Some(self).into(),
			inflight_index: i,
		})
	}

	pub fn submit_read(&self, handle: Handle, buf: Vec<u8>, n: usize) -> Result<Read<'_>, ()> {
		self.submit_read_buffer(buf, handle, n, |buffer| Request::Read { buffer })
			.map(|fut| Read { fut })
	}

	pub fn submit_write(&self, handle: Handle, data: Vec<u8>) -> Result<Write<'_>, ()> {
		self.submit_write_buffer(data, handle, |buffer| Request::Write { buffer })
			.map(|fut| Write { fut })
	}

	pub fn submit_open(&self, handle: Handle, path: Vec<u8>) -> Result<Open<'_>, ()> {
		self.submit_write_buffer(path, handle, |path| Request::Open { path })
			.map(|fut| Open { fut })
	}

	pub fn submit_create(&self, handle: Handle, path: Vec<u8>) -> Result<Create<'_>, ()> {
		self.submit_write_buffer(path, handle, |path| Request::Create { path })
			.map(|fut| Create { fut })
	}

	pub fn submit_seek(&self, handle: Handle, from: SeekFrom) -> Result<Seek<'_>, ()> {
		self.submit_no_buffer(handle, Request::Seek { from })
			.map(|fut| Seek { fut })
	}

	pub fn submit_poll(&self, handle: Handle) -> Result<Poll<'_>, ()> {
		self.submit_no_buffer(handle, Request::Poll)
			.map(|fut| Poll { fut })
	}

	pub fn submit_close(&self, handle: Handle) -> Result<(), ()> {
		self.inner
			.borrow_mut()
			.submit(u64::MAX, handle, Request::Close)
			.map(|b| debug_assert!(!b))
			.map_err(|_| todo!())
	}

	pub fn submit_peek(&self, handle: Handle, buf: Vec<u8>, n: usize) -> Result<Peek<'_>, ()> {
		self.submit_read_buffer(buf, handle, n, |buffer| Request::Peek { buffer })
			.map(|fut| Peek { fut })
	}

	pub fn submit_share(&self, handle: Handle, share: Handle) -> Result<Share<'_>, ()> {
		self.submit_no_buffer(handle, Request::Share { share })
			.map(|fut| Share { fut })
	}

	pub fn process(&self) {
		let mut inner = self.inner.borrow_mut();
		let mut inflight = self.inflight_buffers.borrow_mut();
		while let Some(resp) = inner.receive() {
			let i = arena::Handle::from_raw(resp.user_data as usize, ());
			let s = BufferFutureState::Finished(error::result(resp.value).map(|v| v as u64));
			let t = &mut inflight[i];
			match mem::replace(&mut t.1, s) {
				BufferFutureState::Cancelled => {
					// Remove the buffer to avoid leaks.
					// This is safe since the kernel has finished doing whatever operations it
					// needs to do with it.
					inflight.remove(i).unwrap();
				}
				BufferFutureState::InflightWithWaker(w) => w.wake(),
				_ => {}
			}
		}
	}

	pub fn poll(&self) {
		self.inner.borrow_mut().poll();
	}

	pub fn wait(&self) {
		self.inner.borrow_mut().wait()
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

#[derive(Debug)]
enum BufferFutureState {
	Inflight,
	InflightWithWaker(Waker),
	Finished(error::Result<u64>),
	Cancelled,
}

/// A future that involves byte buffers.
struct BufferFuture<'a> {
	queue: Cell<Option<&'a Queue>>,
	inflight_index: arena::Handle<()>,
}

impl Future for BufferFuture<'_> {
	type Output = (Vec<u8>, error::Result<u64>);

	/// Check if the read request has finished.
	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		let queue = match self.queue.get() {
			Some(q) => q,
			None => panic!("poll after ready"),
		};
		let i = self.inflight_index;
		let mut inflight = queue.inflight_buffers.borrow_mut();
		let t = &mut inflight[i];
		match mem::replace(&mut t.1, BufferFutureState::Cancelled) {
			BufferFutureState::Inflight => {
				t.1 = BufferFutureState::InflightWithWaker(cx.waker().clone());
				TPoll::Pending
			}
			BufferFutureState::InflightWithWaker(waker) => {
				t.1 = BufferFutureState::InflightWithWaker(if waker.will_wake(cx.waker()) {
					waker
				} else {
					cx.waker().clone()
				});
				TPoll::Pending
			}
			BufferFutureState::Finished(res) => {
				let (vec, _) = inflight.remove(i).unwrap();
				self.queue.set(None);
				TPoll::Ready((vec, res))
			}
			BufferFutureState::Cancelled => unreachable!(),
		}
	}
}

impl Drop for BufferFuture<'_> {
	fn drop(&mut self) {
		let queue = match self.queue.get() {
			Some(q) => q,
			None => return,
		};
		let i = self.inflight_index;
		let mut inflight = queue.inflight_buffers.borrow_mut();
		inflight[i].1 = BufferFutureState::Cancelled;
	}
}

/// A pending read request.
pub struct Read<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Read<'_> {
	type Output = Result<Vec<u8>, (Vec<u8>, error::Error)>;

	/// Check if the read request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut)
			.poll(cx)
			.map(|(mut vec, r)| match r {
				Ok(len) => {
					// SAFETY: the kernel should have written (i.e. initialized) at least len bytes
					unsafe { vec.set_len(len as usize) };
					Ok(vec)
				}
				Err(e) => Err((vec, e)),
			})
	}
}

/// A pending write request.
pub struct Write<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Write<'_> {
	type Output = Result<(Vec<u8>, usize), (Vec<u8>, error::Error)>;

	/// Check if the write request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(vec, r)| match r {
			Ok(len) => Ok((vec, len as usize)),
			Err(e) => Err((vec, e)),
		})
	}
}

/// A pending open request.
pub struct Open<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Open<'_> {
	type Output = Result<(Vec<u8>, Handle), (Vec<u8>, error::Error)>;

	/// Check if the open request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(vec, r)| match r {
			Ok(h) => Ok((vec, h as Handle)),
			Err(e) => Err((vec, e)),
		})
	}
}

/// A pending create request.
pub struct Create<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Create<'_> {
	type Output = Result<(Vec<u8>, Handle), (Vec<u8>, error::Error)>;

	/// Check if the create request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(vec, r)| match r {
			Ok(h) => Ok((vec, h as Handle)),
			Err(e) => Err((vec, e)),
		})
	}
}

/// A pending query request.
pub struct Query<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Query<'_> {
	type Output = Result<(Vec<u8>, Handle), (Vec<u8>, error::Error)>;

	/// Check if the query request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(vec, r)| match r {
			Ok(h) => Ok((vec, h as Handle)),
			Err(e) => Err((vec, e)),
		})
	}
}

/// A pending query next request.
pub struct QueryNext<'a> {
	fut: BufferFuture<'a>,
}

impl Future for QueryNext<'_> {
	type Output = Result<Vec<u8>, (Vec<u8>, error::Error)>;

	/// Check if the query request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(vec, r)| match r {
			Ok(_) => Ok(vec),
			Err(e) => Err((vec, e)),
		})
	}
}

/// A pending seek request.
pub struct Seek<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Seek<'_> {
	type Output = Result<u64, error::Error>;

	/// Check if the seek request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(_, r)| r)
	}
}

/// A pending poll request.
pub struct Poll<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Poll<'_> {
	type Output = Result<u64, error::Error>;

	/// Check if the poll request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(_, r)| r)
	}
}

/// A pending peek request.
pub struct Peek<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Peek<'_> {
	type Output = Result<Vec<u8>, (Vec<u8>, error::Error)>;

	/// Check if the peek request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut)
			.poll(cx)
			.map(|(mut vec, r)| match r {
				Ok(len) => {
					// SAFETY: the kernel should have written (i.e. initialized) at least len bytes
					unsafe { vec.set_len(len as usize) };
					Ok(vec)
				}
				Err(e) => Err((vec, e)),
			})
	}
}

/// A pending share request.
pub struct Share<'a> {
	fut: BufferFuture<'a>,
}

impl Future for Share<'_> {
	type Output = Result<u64, error::Error>;

	/// Check if the share request has finished.
	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> TPoll<Self::Output> {
		Pin::new(&mut self.fut).poll(cx).map(|(_, r)| r)
	}
}
