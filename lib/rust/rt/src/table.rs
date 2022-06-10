#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use super::io;
use alloc::vec::Vec;
use core::{
	fmt,
	marker::PhantomData,
	mem::{self, MaybeUninit},
};

pub use norostb_kernel::{io::Job, Handle};

#[derive(Debug)]
pub struct Object(Handle);

impl Object {
	#[inline]
	pub fn open(&self, path: &[u8]) -> io::Result<Self> {
		io::block_on(io::open(self.0, path.into(), 0))
			.map(|(_, h)| Self(h))
			.map_err(|(_, e)| e)
	}

	#[inline]
	pub fn create(&self, path: &[u8]) -> io::Result<Self> {
		io::block_on(io::create(self.0, path.into(), 0))
			.map(|(_, h)| Self(h))
			.map_err(|(_, e)| e)
	}

	#[inline]
	pub fn read_vec(&self, amount: usize) -> io::Result<Vec<u8>> {
		io::block_on(io::read(self.0, Vec::new(), amount)).map_err(|(_, e)| e)
	}

	#[inline]
	pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		self.read_vec(buf.len()).map(|b| {
			buf[..b.len()].copy_from_slice(&b);
			b.len()
		})
	}

	#[inline]
	pub fn read_uninit<'a>(
		&self,
		buf: &'a mut [MaybeUninit<u8>],
	) -> io::Result<(&'a mut [u8], &'a mut [MaybeUninit<u8>])> {
		self.read_vec(buf.len()).map(|b| {
			let (i, u) = buf.split_at_mut(b.len());
			(MaybeUninit::write_slice(i, &b), u)
		})
	}

	#[inline]
	pub fn peek_vec(&self, amount: usize) -> io::Result<Vec<u8>> {
		io::block_on(io::peek(self.0, Vec::new(), amount)).map_err(|(_, e)| e)
	}

	#[inline]
	pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
		self.peek_vec(buf.len()).map(|b| {
			buf[..b.len()].copy_from_slice(&b);
			b.len()
		})
	}

	#[inline]
	pub fn peek_uninit<'a>(
		&self,
		buf: &'a mut [MaybeUninit<u8>],
	) -> io::Result<(&'a mut [u8], &'a mut [MaybeUninit<u8>])> {
		self.peek_vec(buf.len()).map(|b| {
			let (i, u) = buf.split_at_mut(b.len());
			(MaybeUninit::write_slice(i, &b), u)
		})
	}

	#[inline]
	pub fn write_vec(&self, data: Vec<u8>, offset: usize) -> io::Result<usize> {
		io::block_on(io::write(self.0, data, offset))
			.map(|(_, l)| l)
			.map_err(|(_, e)| e)
	}

	#[inline]
	pub fn write(&self, data: &[u8]) -> io::Result<usize> {
		self.write_vec(data.into(), 0)
	}

	#[inline]
	pub fn seek(&self, pos: io::SeekFrom) -> io::Result<u64> {
		io::block_on(io::seek(self.0, pos))
	}

	#[inline]
	pub fn share(&self, share: &Object) -> io::Result<u64> {
		io::block_on(io::share(self.0, share.0))
	}

	#[inline]
	pub fn poll(&self) -> io::Result<u64> {
		io::block_on(io::poll(self.0))
	}

	#[inline]
	pub fn duplicate(&self) -> io::Result<Self> {
		io::duplicate(self.0).map(Self)
	}

	#[inline]
	pub fn create_root() -> io::Result<Self> {
		io::create_root().map(Self)
	}

	#[inline]
	pub const fn as_raw(&self) -> Handle {
		self.0
	}

	#[inline]
	pub const fn into_raw(self) -> Handle {
		let h = self.0;
		mem::forget(self);
		h
	}

	#[inline(always)]
	pub fn as_ref_object(&self) -> RefObject<'_> {
		RefObject {
			handle: self.0,
			_marker: PhantomData,
		}
	}

	#[inline]
	pub const fn from_raw(handle: Handle) -> Self {
		Self(handle)
	}

	/// Convienence method for use with `write!()` et al.
	pub fn write_fmt(&self, args: fmt::Arguments<'_>) -> io::Result<()> {
		struct Fmt {
			buf: Vec<u8>,
			obj: Handle,
			res: io::Result<()>,
		}
		impl fmt::Write for Fmt {
			fn write_str(&mut self, s: &str) -> fmt::Result {
				self.buf.clear();
				self.buf.extend_from_slice(s.as_bytes());
				// FIXME we need some kind of write_all
				match io::block_on(io::write(self.obj, mem::take(&mut self.buf), 0)) {
					Ok((buf, _len)) => Ok(self.buf = buf),
					Err((buf, e)) => {
						self.buf = buf;
						self.res = Err(e);
						Err(fmt::Error)
					}
				}
			}
		}
		let mut f = Fmt {
			buf: Vec::new(),
			obj: self.0,
			res: Ok(()),
		};
		let _ = fmt::write(&mut f, args);
		f.res
	}
}

impl Drop for Object {
	fn drop(&mut self) {
		io::close(self.0)
	}
}

/// An object by "reference" but with less indirection.
#[derive(Clone, Copy)]
pub struct RefObject<'a> {
	handle: Handle,
	_marker: PhantomData<&'a Object>,
}

impl<'a> RefObject<'a> {
	#[inline]
	pub const fn from_raw(handle: Handle) -> Self {
		Self {
			handle,
			_marker: PhantomData,
		}
	}

	#[inline]
	pub const fn as_raw(&self) -> Handle {
		self.handle
	}

	#[inline]
	pub const fn into_raw(self) -> Handle {
		self.handle
	}
}

impl<'a> core::ops::Deref for RefObject<'a> {
	type Target = Object;

	fn deref(&self) -> &Self::Target {
		// SAFETY: Object is a simple wrapper around the handle.
		unsafe { mem::transmute(&self.handle) }
	}
}
