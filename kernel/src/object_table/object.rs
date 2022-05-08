use super::{Error, MemoryObject, Query, Table, Ticket};
use alloc::{boxed::Box, sync::Arc, vec::Vec};

/// A single object.
pub trait Object {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(self: Arc<Self>, _offset: u64) -> Option<Arc<dyn MemoryObject>> {
		None
	}

	/// Search for objects based on a name and/or tags.
	fn query(self: Arc<Self>, _prefix: Vec<u8>, _path: &[u8]) -> Ticket<Box<dyn Query>> {
		not_implemented()
	}

	/// Open a single object based on path.
	fn open(self: Arc<Self>, _path: &[u8]) -> Ticket<Arc<dyn Object>> {
		not_implemented()
	}

	/// Create a new object with the given path.
	fn create(self: Arc<Self>, _path: &[u8]) -> Ticket<Arc<dyn Object>> {
		not_implemented()
	}

	fn read(&self, _offset: u64, _length: usize) -> Ticket<Box<[u8]>> {
		not_implemented()
	}

	fn peek(&self, _offset: u64, _length: usize) -> Ticket<Box<[u8]>> {
		not_implemented()
	}

	fn write(&self, _offset: u64, _data: &[u8]) -> Ticket<usize> {
		not_implemented()
	}

	fn seek(&self, _from: norostb_kernel::io::SeekFrom) -> Ticket<u64> {
		not_implemented()
	}

	fn poll(&self) -> Ticket<usize> {
		not_implemented()
	}

	fn share(&self, _object: &Arc<dyn Object>) -> Ticket<u64> {
		not_implemented()
	}

	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		None
	}
}

fn not_implemented<T>() -> Ticket<T> {
	Ticket::new_complete(Err(Error::InvalidOperation))
}
