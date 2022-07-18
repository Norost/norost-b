use crate::{io, queue};
use core::{marker::PhantomData, mem, ops::Deref};

#[repr(transparent)]
pub struct AsyncObject(rt::Handle);

impl From<rt::Object> for AsyncObject {
	fn from(obj: rt::Object) -> Self {
		Self(obj.into_raw())
	}
}

impl<B: io::BufMut> io::Read<B> for AsyncObject {
	type Future = io_queue_rt::Read<'static, B>;

	fn read(&self, buf: B) -> Self::Future {
		queue::submit(|q, b| q.submit_read(self.0, b), buf)
	}
}

impl<B: io::Buf> io::Write<B> for AsyncObject {
	type Future = io_queue_rt::Write<'static, B>;

	fn write(&self, buf: B) -> Self::Future {
		queue::submit(|q, b| q.submit_write(self.0, b), buf)
	}
}

macro_rules! impl_wrap {
	($ty:ident read) => {
		impl<B: crate::io::BufMut> crate::io::Read<B> for $ty {
			type Future = <AsyncObject as crate::io::Read<B>>::Future;

			fn read(&self, buf: B) -> Self::Future {
				self.0.read(buf)
			}
		}
	};
	($ty:ident write) => {
		impl<B: crate::io::Buf> crate::io::Write<B> for $ty {
			type Future = <AsyncObject as crate::io::Write<B>>::Future;

			fn write(&self, buf: B) -> Self::Future {
				self.0.write(buf)
			}
		}
	};
}

impl Drop for AsyncObject {
	fn drop(&mut self) {
		let _ = queue::get()
			.submit_close(self.0)
			.expect("todo: wrapper that blocks");
	}
}

/// An object by "reference" but with less indirection.
#[derive(Clone, Copy)]
pub struct RefAsyncObject<'a> {
	handle: rt::Handle,
	_marker: PhantomData<&'a AsyncObject>,
}

impl<'a> From<&'a rt::Object> for RefAsyncObject<'a> {
	fn from(obj: &'a rt::Object) -> Self {
		Self::from(obj.as_ref_object())
	}
}

impl<'a> From<rt::RefObject<'a>> for RefAsyncObject<'a> {
	fn from(obj: rt::RefObject<'a>) -> Self {
		Self {
			handle: obj.as_raw(),
			_marker: PhantomData,
		}
	}
}

impl<'a> Deref for RefAsyncObject<'a> {
	type Target = AsyncObject;

	fn deref(&self) -> &Self::Target {
		// SAFETY: Object is a simple wrapper around the handle.
		unsafe { mem::transmute(&self.handle) }
	}
}
