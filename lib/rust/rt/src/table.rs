use crate::io::{self, TinySlice, RWX};
use core::{
	fmt,
	marker::PhantomData,
	mem::{self, MaybeUninit},
	ptr::NonNull,
};

pub use norostb_kernel::{io::DoIo, object::NewObject, Handle};

#[derive(Debug)]
pub struct Object(Handle);

impl Object {
	/// Create a new local object.
	#[inline(always)]
	pub fn new(args: NewObject) -> io::Result<Self> {
		io::new_object(args).map(Self)
	}

	#[inline(always)]
	pub fn open(&self, path: &[u8]) -> io::Result<Self> {
		io::open(self.0, path).map(Self)
	}

	#[inline(always)]
	pub fn create(&self, path: &[u8]) -> io::Result<Self> {
		io::create(self.0, path).map(Self)
	}

	#[inline(always)]
	pub fn destroy(&self, path: &[u8]) -> io::Result<u64> {
		io::destroy(self.0, path)
	}

	#[inline(always)]
	pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		io::read(self.0, buf)
	}

	#[inline]
	pub fn read_uninit<'a>(
		&self,
		buf: &'a mut [MaybeUninit<u8>],
	) -> io::Result<(&'a mut [u8], &'a mut [MaybeUninit<u8>])> {
		io::read_uninit(self.0, buf).map(|l| {
			let (i, u) = buf.split_at_mut(l);
			// SAFETY: all bytes in i are initialized
			(unsafe { MaybeUninit::slice_assume_init_mut(i) }, u)
		})
	}

	#[inline(always)]
	pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
		io::peek(self.0, buf)
	}

	#[inline]
	pub fn peek_uninit<'a>(
		&self,
		buf: &'a mut [MaybeUninit<u8>],
	) -> io::Result<(&'a mut [u8], &'a mut [MaybeUninit<u8>])> {
		io::peek_uninit(self.0, buf).map(|l| {
			let (i, u) = buf.split_at_mut(l);
			// SAFETY: all bytes in i are initialized
			(unsafe { MaybeUninit::slice_assume_init_mut(i) }, u)
		})
	}

	#[inline]
	pub fn write(&self, data: &[u8]) -> io::Result<usize> {
		io::write(self.0, data)
	}

	#[inline]
	pub fn get_meta(
		&self,
		property: &TinySlice<u8>,
		value: &mut TinySlice<u8>,
	) -> io::Result<usize> {
		io::get_meta(self.0, property, value)
	}

	#[inline]
	pub fn get_meta_uninit(
		&self,
		property: &TinySlice<u8>,
		value: &mut TinySlice<MaybeUninit<u8>>,
	) -> io::Result<usize> {
		io::get_meta_uninit(self.0, property, value)
	}

	#[inline]
	pub fn set_meta(&self, property: &TinySlice<u8>, value: &TinySlice<u8>) -> io::Result<usize> {
		io::set_meta(self.0, property, value)
	}

	#[inline]
	pub fn seek(&self, pos: io::SeekFrom) -> io::Result<u64> {
		io::seek(self.0, pos)
	}

	#[inline]
	pub fn share(&self, share: &Object) -> io::Result<u64> {
		io::share(self.0, share.0)
	}

	#[inline]
	pub fn map_object(
		&self,
		base: Option<NonNull<u8>>,
		rwx: RWX,
		offset: usize,
		max_length: usize,
	) -> io::Result<(NonNull<u8>, usize)> {
		io::map_object(self.0, base, rwx, offset, max_length)
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

	#[inline]
	pub const fn from_raw(handle: Handle) -> Self {
		Self(handle)
	}

	/// Convienence method for use with `write!()` et al.
	pub fn write_fmt(&self, args: fmt::Arguments<'_>) -> io::Result<()> {
		struct Fmt {
			obj: Handle,
			res: io::Result<()>,
		}
		impl fmt::Write for Fmt {
			fn write_str(&mut self, s: &str) -> fmt::Result {
				io::write(self.obj, s.as_bytes()).map(|_| ()).map_err(|e| {
					self.res = Err(e);
					fmt::Error
				})
			}
		}
		let mut f = Fmt {
			obj: self.0,
			res: Ok(()),
		};
		let _ = fmt::write(&mut f, args);
		f.res
	}
}

impl Drop for Object {
	/// Close the handle to this object.
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
	pub const fn from_raw(handle: Handle) -> Self {
		Self {
			handle,
			_marker: PhantomData,
		}
	}

	pub const fn as_raw(&self) -> Handle {
		self.handle
	}

	pub const fn into_raw(self) -> Handle {
		self.handle
	}
}

impl<'a> From<&'a Object> for RefObject<'a> {
	fn from(obj: &'a Object) -> Self {
		Self {
			handle: obj.0,
			_marker: PhantomData,
		}
	}
}

impl<'a> core::ops::Deref for RefObject<'a> {
	type Target = Object;

	fn deref(&self) -> &Self::Target {
		// SAFETY: Object is a simple wrapper around the handle.
		unsafe { mem::transmute(&self.handle) }
	}
}
