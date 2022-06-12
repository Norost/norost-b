use super::{PPNBox, PageFrameIter, PPN};
use crate::memory::frame;
use crate::scheduler::MemoryObject;
use core::num::NonZeroUsize;

/// A physically contiguous range of pages
pub struct DMAFrame {
	base: PPN,
	count: PPNBox,
}

impl DMAFrame {
	pub fn new(count: PPNBox) -> Result<Self, frame::AllocateContiguousError> {
		frame::allocate_contiguous(NonZeroUsize::new(count.try_into().unwrap()).unwrap()).map(
			|base| {
				unsafe {
					base.as_ptr().write_bytes(0, count.try_into().unwrap());
				}
				Self { base, count }
			},
		)
	}
}

unsafe impl MemoryObject for DMAFrame {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN])) {
		(0..self.count)
			.map(|i| self.base.skip(i))
			.for_each(|p| f(&[p]));
	}

	fn physical_pages_len(&self) -> usize {
		self.count.try_into().unwrap()
	}
}

impl Drop for DMAFrame {
	fn drop(&mut self) {
		let mut iter = PageFrameIter {
			base: self.base,
			count: self.count.try_into().unwrap(),
		};
		unsafe {
			super::deallocate(self.count.try_into().unwrap(), || iter.next().unwrap()).unwrap();
		}
	}
}
