use super::{Error, MemoryObject, Table, Ticket};
use alloc::{boxed::Box, sync::Arc};

/// A single object.
pub trait Object {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(&self, _offset: u64) -> Option<Box<dyn MemoryObject>> {
		None
	}

	fn read(&self, _offset: u64, _length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn write(&self, _offset: u64, _data: &[u8]) -> Ticket<usize> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn seek(&self, _from: norostb_kernel::io::SeekFrom) -> Ticket<u64> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn poll(&self) -> Ticket<usize> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		None
	}
}
