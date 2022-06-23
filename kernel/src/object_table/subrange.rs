use super::{MemoryObject, Object};
use crate::memory::{frame::PPN, Page};
use alloc::sync::Arc;
use core::ops::RangeInclusive;

/// A subrange of a memory mappable object.
pub struct SubRange {
	/// The object to be mapped.
	object: Arc<dyn MemoryObject>,
	/// The offset from the start of the object when mapping in pages.
	start_offset: usize,
	/// How many pages in total to map at most.
	total_size: usize,
}

impl SubRange {
	pub fn new(
		object: Arc<dyn MemoryObject>,
		range: RangeInclusive<usize>,
	) -> Result<Arc<Self>, NewSubRangeError> {
		if range.start() > range.end() {
			Err(NewSubRangeError::BadRange)
		} else if *range.start() & Page::MASK != 0 || *range.end() & Page::MASK != Page::MASK {
			Err(NewSubRangeError::BadAlignment)
		} else {
			Ok(Arc::new(Self {
				object,
				start_offset: *range.start(),
				total_size: *range.end() - *range.start(),
			}))
		}
	}
}

impl Object for SubRange {
	fn memory_object(self: Arc<Self>, _offset: u64) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for SubRange {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		let mut offset @ mut total = 0;
		let mut f = |mut p: &[PPN]| {
			if total >= self.total_size {
				false
			} else if offset + p.len() < self.start_offset {
				offset += p.len();
				true
			} else {
				if offset < self.start_offset {
					p = &p[self.start_offset - offset..];
				} else if total + p.len() > self.total_size {
					p = &p[..self.total_size - total]
				}
				total += p.len();
				f(p)
			}
		};
		self.object.physical_pages(&mut f);
	}

	fn physical_pages_len(&self) -> usize {
		self.total_size
	}
}

pub enum NewSubRangeError {
	BadAlignment,
	BadRange,
}
