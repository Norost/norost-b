use super::{AllocateError, AllocateHints, Page, PageFrame};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, vec::Vec};
use core::{num::NonZeroUsize, ops::Range};

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
		super::allocate(
			size.get(),
			|f| {
				unsafe {
					assert_eq!(f.p2size, 0, "TODO");
					f.base.as_ptr().write_bytes(0, 1 << f.p2size);
				}
				frames.push(f);
			},
			hints.address,
			hints.color,
		)?;
		Ok(Self {
			frames: frames.into(),
		})
	}

	pub unsafe fn clear(&mut self) {
		for f in self.frames.iter() {
			unsafe {
				f.base.as_ptr().cast::<Page>().write_bytes(0, 1 << f.p2size);
			}
		}
	}

	pub unsafe fn write(&self, start: usize, data: &[u8]) {
		dbg!(start, data.len());
		// FIXME don't assume page frames are 0 p2size each.
		for (i, b) in (start..).zip(data) {
			let frame = &self.frames[i / Page::SIZE];
			// FIXME unsynchronized writes are UB.
			unsafe {
				*frame.base.as_ptr().cast::<u8>().add(i % Page::SIZE) = *b;
			}
		}
	}
}

impl MemoryObject for OwnedPageFrames {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		self.frames.clone()
	}
}

impl Drop for OwnedPageFrames {
	fn drop(&mut self) {
		unsafe {
			let mut i = 0;
			super::deallocate(self.frames.len(), || {
				let f = self.frames[i];
				i += 1;
				f
			})
			.unwrap();
		}
	}
}
