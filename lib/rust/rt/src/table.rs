use super::io;
use core::mem::{self, MaybeUninit};

pub use norostb_kernel::{
	io::{Job, ObjectInfo},
	Handle,
};

#[derive(Debug)]
pub struct Object(Handle);

impl Object {
	#[inline]
	pub fn open(&self, path: &[u8]) -> io::Result<Self> {
		io::open(self.0, path).map(Self)
	}

	#[inline]
	pub fn create(&self, path: &[u8]) -> io::Result<Self> {
		io::create(self.0, path).map(Self)
	}

	#[inline]
	pub fn query(&self, path: &[u8]) -> io::Result<io::Query> {
		io::query(self.0, path).map(io::Query)
	}

	#[inline]
	pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
		io::read(self.0, buf)
	}

	#[inline]
	pub fn read_uninit(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		io::read_uninit(self.0, buf)
	}

	#[inline]
	pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
		io::peek(self.0, buf)
	}

	#[inline]
	pub fn peek_uninit(&self, buf: &mut [MaybeUninit<u8>]) -> io::Result<usize> {
		io::peek_uninit(self.0, buf)
	}

	#[inline]
	pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
		io::write(self.0, buf)
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
	pub fn duplicate(&self) -> io::Result<Self> {
		io::duplicate(self.0).map(Self)
	}

	#[inline]
	pub fn take_job(&self, job: &mut Job) -> io::Result<()> {
		io::take_job(self.0, job)
	}

	#[inline]
	pub fn finish_job(&self, job: &Job) -> io::Result<()> {
		io::finish_job(self.0, job)
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

	#[inline]
	pub const fn from_raw(handle: Handle) -> Self {
		Self(handle)
	}
}

impl Drop for Object {
	fn drop(&mut self) {
		io::close(self.0)
	}
}
