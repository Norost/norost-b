use super::{PPNBox, PageFrame, PPN};
use crate::memory::frame;
use crate::scheduler::MemoryObject;
use alloc::boxed::Box;
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

impl MemoryObject for DMAFrame {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		(0..self.count)
			.map(|i| PageFrame {
				base: self.base.skip(i),
				p2size: 0,
			})
			.collect()
	}
}
