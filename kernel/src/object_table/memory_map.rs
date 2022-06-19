use super::{MemoryObject, Object};
use crate::memory::frame::PPN;
use alloc::{boxed::Box, sync::Arc};

/// A "memory mapped" range of objects.
pub struct MemoryMap {
	/// All objects that are shared.
	objects: Box<[Arc<dyn MemoryObject>]>,
	/// The offset from the start of the first object when mapping in pages.
	start_offset: usize,
	/// How much to map of the last object at most in pages.
	end_size: usize,
}

impl MemoryMap {
	pub fn new(
		objects: impl Into<Box<[Arc<dyn MemoryObject>]>>,
		start_offset: usize,
		end_size: usize,
	) -> Self {
		Self {
			objects: objects.into(),
			start_offset,
			end_size,
		}
	}
}

impl Object for MemoryMap {
	fn memory_object(self: Arc<Self>, _offset: u64) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for MemoryMap {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN])) {
		self.objects.iter().for_each(|o| o.physical_pages(f))
	}

	fn physical_pages_len(&self) -> usize {
		let mut it = self.objects.iter();
		self.objects.iter().map(|o| o.physical_pages_len()).sum()
	}
}
