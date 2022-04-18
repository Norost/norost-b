use super::{AllocateError, AllocateHints, PageFrame};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, vec::Vec};
use core::num::NonZeroUsize;

/// An allocation of page frames.
pub struct OwnedPageFrames {
	frames: Box<[PageFrame]>,
}

impl OwnedPageFrames {
	pub fn new(size: NonZeroUsize, hints: AllocateHints) -> Result<Self, AllocateError> {
		let mut frames = Vec::new();
		// FIXME if we don't pre allocate we will deadlock.
		// This is a shitty workaround
		frames.reserve(size.get());
		super::allocate(size.get(), |f| frames.push(f), hints.address, hints.color)?;
		Ok(Self {
			frames: frames.into(),
		})
	}
}

impl MemoryObject for OwnedPageFrames {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		self.frames.clone()
	}
}
