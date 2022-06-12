//! Bitmap-based allocator. Intended for DMA allocations.

use super::{MemoryRegion, NonZeroUsize, PPN};

pub(super) struct FixedBitmap {
	base: PPN,
	bitmap: u128, // 512K is enough for now.
}

impl FixedBitmap {
	pub fn new_from_region(mr: &mut MemoryRegion) -> Option<Self> {
		mr.count.checked_sub(128).map(|count| {
			let base = mr.base;
			mr.base = base.skip(128);
			mr.count = count;
			Self {
				base,
				bitmap: u128::MAX,
			}
		})
	}

	pub fn pop_range(&mut self, n: NonZeroUsize) -> Option<PPN> {
		if n.get() > 128 {
			return None;
		}
		let mask = (1u128 << n.get()).wrapping_sub(1);
		let mut shift = 0;
		while (self.bitmap >> shift) & mask != mask {
			shift += 1;
			if shift == 128 {
				return None;
			}
		}
		self.bitmap &= !(mask << shift);
		Some(self.base.skip(shift))
	}
}
