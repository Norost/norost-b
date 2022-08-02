//! Wrapper around [`Read`]/[`Write`] objects that implements [`AsyncRead`]/[`AsyncWrite`]
//! respectively.
//!
//! Using [`Read`]/[`Write`] directly is recommended to avoid redundant allocations & copies.

use crate::io::{Read, Write};
use alloc::vec::Vec;
use core::{
	future::Future,
	pin::Pin,
	task::{ready, Context, Poll},
};
use futures_io::{AsyncRead, AsyncWrite, Error, ErrorKind};

#[pin_project::pin_project]
pub struct AsyncWrapR<T: Read<Vec<u8>>> {
	io: T,
	#[pin]
	read: ReadState<T>,
}

impl<T> AsyncWrapR<T>
where
	T: Read<Vec<u8>>,
{
	pub fn new(io: T) -> Self {
		Self {
			io,
			read: ReadState::Idle,
		}
	}
}

impl<T> AsyncRead for AsyncWrapR<T>
where
	T: Read<Vec<u8>>,
{
	fn poll_read(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<Result<usize, Error>> {
		let mut slf = self.as_mut().project();
		slf.read.poll_read(&mut slf.io, cx, buf)
	}
}

#[pin_project::pin_project]
pub struct AsyncWrapW<T: Write<Vec<u8>> + Write<()>> {
	io: T,
	#[pin]
	write: WriteState<T>,
}

impl<T> AsyncWrapW<T>
where
	T: Write<Vec<u8>> + Write<()>,
{
	pub fn new(io: T) -> Self {
		Self {
			io,
			write: WriteState::Idle,
		}
	}
}

impl<T> AsyncWrite for AsyncWrapW<T>
where
	T: Write<Vec<u8>> + Write<()>,
{
	fn poll_write(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		data: &[u8],
	) -> Poll<Result<usize, Error>> {
		let mut slf = self.as_mut().project();
		slf.write.poll_write(&mut slf.io, cx, data)
	}

	fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Error>> {
		todo!()
	}

	fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Error>> {
		todo!()
	}
}

#[pin_project::pin_project]
pub struct AsyncWrapRW<T: Read<Vec<u8>> + Write<Vec<u8>> + Write<()>> {
	io: T,
	#[pin]
	read: ReadState<T>,
	#[pin]
	write: WriteState<T>,
}

impl<T> AsyncWrapRW<T>
where
	T: Read<Vec<u8>> + Write<Vec<u8>> + Write<()>,
{
	pub fn new(io: T) -> Self {
		Self {
			io,
			read: ReadState::Idle,
			write: WriteState::Idle,
		}
	}
}

impl<T> AsyncRead for AsyncWrapRW<T>
where
	T: Read<Vec<u8>> + Write<Vec<u8>> + Write<()>,
{
	fn poll_read(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<Result<usize, Error>> {
		let mut slf = self.as_mut().project();
		slf.read.poll_read(&mut slf.io, cx, buf)
	}
}

impl<T> AsyncWrite for AsyncWrapRW<T>
where
	T: Read<Vec<u8>> + Write<Vec<u8>> + Write<()>,
{
	fn poll_write(
		mut self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		data: &[u8],
	) -> Poll<Result<usize, Error>> {
		let mut slf = self.as_mut().project();
		slf.write.poll_write(&mut slf.io, cx, data)
	}

	fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Error>> {
		todo!()
	}

	fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Error>> {
		todo!()
	}
}

#[pin_project::pin_project(project = ReadStateProj)]
enum ReadState<T: Read<Vec<u8>>> {
	Idle,
	Wait(#[pin] T::Future),
	Ready { data: Vec<u8>, offset: usize },
}

impl<T: Read<Vec<u8>>> ReadState<T> {
	fn poll_read(
		mut self: Pin<&mut Self>,
		io: &mut T,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<Result<usize, Error>> {
		loop {
			match self.as_mut().project() {
				ReadStateProj::Idle => {
					let v = Vec::with_capacity(buf.len());
					self.set(Self::Wait(io.read(v)));
				}
				ReadStateProj::Wait(fut) => {
					let (res, data) = ready!(fut.poll(cx));
					if let Err(res) = res {
						return Poll::Ready(Err(err_rt_to_std(res)));
					}
					if data.is_empty() {
						self.set(Self::Idle);
						return Poll::Ready(Ok(0));
					}
					self.set(Self::Ready { data, offset: 0 });
				}
				ReadStateProj::Ready { data, offset } => {
					let data = &data[*offset..];
					let len = data.len().min(buf.len());
					debug_assert_ne!(len, 0, "can't eof here");
					buf[..len].copy_from_slice(&data[..len]);
					*offset += len;
					if *offset == data.len() {
						self.set(Self::Idle);
					}
					return Poll::Ready(Ok(len));
				}
			}
		}
	}
}

#[pin_project::pin_project(project = WriteStateProj)]
enum WriteState<T: Write<Vec<u8>> + Write<()>> {
	Idle,
	Wait(#[pin] <T as Write<()>>::Future),
}

impl<T: Write<Vec<u8>> + Write<()>> WriteState<T> {
	// FIXME proper non-blocking async write
	// There is no way to poll for "write readiness", nor will such an API ever be added.
	// We can sortof work around this by tracking when an empty write succeeds, but it's flaky.
	fn poll_write(
		mut self: Pin<&mut Self>,
		io: &mut T,
		cx: &mut Context<'_>,
		data: &[u8],
	) -> Poll<Result<usize, Error>> {
		loop {
			match self.as_mut().project() {
				WriteStateProj::Idle => {
					// Empty write that doesn't block, which acts as a readiness check
					self.set(WriteState::Wait(io.write(())));
				}
				WriteStateProj::Wait(fut) => {
					let (res, ()) = ready!(fut.poll(cx));
					self.set(WriteState::Idle);
					if let Err(e) = res {
						return Poll::Ready(Err(err_rt_to_std(e)));
					}
					let fut: <T as Write<Vec<u8>>>::Future = io.write(Vec::from(data));
					futures_lite::pin!(fut);
					loop {
						if let Poll::Ready((res, _)) = fut.as_mut().poll(cx) {
							return Poll::Ready(res.map_err(err_rt_to_std));
						}
						// Block until ready
						// Not ideal but there's not much we can do.
						crate::queue::wait(core::time::Duration::MAX);
					}
				}
			}
		}
	}
}

fn err_rt_to_std(_e: rt::Error) -> Error {
	Error::new(ErrorKind::Other, "todo: map error")
}
