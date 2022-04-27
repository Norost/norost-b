use super::io;
use core::mem::{ManuallyDrop, MaybeUninit};
use norostb_kernel::syscall;

pub use norostb_kernel::{
	io::ObjectInfo,
	syscall::{TableId, TableInfo},
	Handle,
};

#[derive(Debug)]
pub struct Object(Handle);

impl Object {
	#[inline]
	pub fn open(table: TableId, path: &[u8]) -> io::Result<Self> {
		// Find a unique ID
		io::open(table, path).map(Self)
	}

	#[inline]
	pub fn create(table: TableId, path: &[u8]) -> io::Result<Self> {
		io::create(table, path).map(Self)
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
	pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
		io::write(self.0, buf)
	}

	pub fn seek(&self, pos: io::SeekFrom) -> io::Result<u64> {
		io::seek(self.0, pos)
	}

	pub fn duplicate(&self) -> io::Result<Self> {
		io::duplicate(self.0).map(Self)
	}

	pub fn as_raw(&self) -> Handle {
		self.0
	}

	pub fn into_raw(self) -> Handle {
		ManuallyDrop::new(self).0
	}

	pub fn from_raw(handle: Handle) -> Self {
		Self(handle)
	}
}

impl Drop for Object {
	fn drop(&mut self) {
		io::close(self.0)
	}
}

/// An iterator over all tables.
#[derive(Debug)]
pub struct TableIter {
	state: Option<Option<TableId>>,
}

impl TableIter {
	/// Create an iterator over all tables.
	#[inline(always)]
	pub fn new() -> super::io::Result<Self> {
		Ok(Self { state: Some(None) })
	}
}

impl Iterator for TableIter {
	type Item = (TableId, TableInfo);

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		self.state
			.take()
			.and_then(|id| syscall::next_table(id))
			.map(|(id, info)| {
				self.state = Some(Some(id));
				(id, info)
			})
	}
}
