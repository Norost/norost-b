use {
	super::{Error, MemoryObject, Ticket, TinySlice},
	alloc::{boxed::Box, sync::Arc},
};

/// A single object.
pub trait Object {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		None
	}

	/// Open a single object based on path.
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		let _ = path;
		not_implemented()
	}

	/// Create a new object with the given path.
	fn create(self: Arc<Self>, _path: &[u8]) -> Ticket<Arc<dyn Object>> {
		not_implemented()
	}

	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let _ = length;
		not_implemented()
	}

	fn write(self: Arc<Self>, _data: &[u8]) -> Ticket<u64> {
		not_implemented()
	}

	fn seek(&self, _from: norostb_kernel::io::SeekFrom) -> Ticket<u64> {
		not_implemented()
	}

	fn share(&self, _object: &Arc<dyn Object>) -> Ticket<u64> {
		not_implemented()
	}

	fn destroy(&self, _path: &[u8]) -> Ticket<u64> {
		not_implemented()
	}

	/// Get meta-information about an object.
	fn get_meta(self: Arc<Self>, property: &TinySlice<u8>) -> Ticket<Box<[u8]>> {
		let _ = property;
		not_implemented()
	}

	/// Get meta-information about an object.
	fn set_meta(self: Arc<Self>, property: &TinySlice<u8>, value: &TinySlice<u8>) -> Ticket<u64> {
		let _ = (property, value);
		not_implemented()
	}
}

fn not_implemented<T>() -> Ticket<T> {
	Ticket::new_complete(Err(Error::InvalidOperation))
}
