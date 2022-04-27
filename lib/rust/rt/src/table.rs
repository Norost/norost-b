use core::mem::ManuallyDrop;

use norostb_kernel::syscall;
pub use norostb_kernel::{
	io::ObjectInfo,
	syscall::{TableId, TableInfo},
	Handle,
};

pub struct Object(Handle);

impl Object {
	pub fn into_raw(self) -> Handle {
		ManuallyDrop::new(self).0
	}

	pub fn from_raw(handle: Handle) -> Self {
		Self(handle)
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
