use super::{MemoryObject, Object};
use crate::memory::frame::PPN;
use alloc::{boxed::Box, sync::Arc};

/// A "memory mapped" range of objects.
pub struct MemoryMap {
	/// All objects that are shared.
	objects: Box<[Arc<dyn MemoryObject>]>,
	/// The offset from the start of the first object when mapping in pages.
	start_offset: usize,
	/// How many pages in total this [`MemoryMap`] encompasses.
	total_size: usize,
}

impl MemoryMap {
	pub fn new(
		objects: Box<[Arc<dyn MemoryObject>]>,
		start_offset: usize,
		total_size: usize,
	) -> Self {
		assert!(!objects.is_empty(), "there must be objects");
		Self {
			objects: objects.into(),
			start_offset,
			total_size,
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
		let mut offset @ mut total = 0;
		let mut f = |mut p: &[PPN]| {
			if total >= self.total_size {
				return;
			}
			if offset + p.len() < self.start_offset {
				offset += p.len();
				return;
			}
			if offset < self.start_offset {
				p = &p[self.start_offset - offset..];
			}
			if total + p.len() > self.total_size {
				p = &p[..self.total_size - total]
			}
			total += p.len();
			f(p)
		};
		self.objects.iter().for_each(|o| o.physical_pages(&mut f));
	}

	fn physical_pages_len(&self) -> usize {
		self.total_size
	}
}
