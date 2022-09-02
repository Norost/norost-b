use {
	super::{AllocateError, AllocateHints, Page, PPN},
	crate::{
		memory::r#virtual::RWX,
		object_table::{Object, PageFlags},
		scheduler::MemoryObject,
	},
	alloc::{boxed::Box, sync::Arc, vec::Vec},
	core::num::NonZeroUsize,
};

/// An allocation of page frames.
pub struct OwnedPageFrames {
	frames: Box<[PPN]>,
}

impl OwnedPageFrames {
	pub fn new(size: NonZeroUsize, hints: AllocateHints) -> Result<Self, AllocateError> {
		let mut frames = Vec::new();
		// FIXME if we don't pre allocate we will deadlock.
		// This is a shitty workaround
		frames.reserve(size.get());
		super::allocate(size.get(), |f| frames.push(f), hints.address, hints.color)?;
		Ok({
			let mut s = Self { frames: frames.into() };
			unsafe { s.clear() };
			s
		})
	}

	pub unsafe fn clear(&mut self) {
		for f in self.frames.iter() {
			unsafe {
				f.as_ptr().cast::<Page>().write_bytes(0, 1);
			}
		}
	}

	pub unsafe fn write(&self, start: usize, data: &[u8]) {
		for (i, b) in (start..).zip(data) {
			let frame = &self.frames[i / Page::SIZE];
			// FIXME unsynchronized writes are UB.
			unsafe {
				*frame.as_ptr().cast::<u8>().add(i % Page::SIZE) = *b;
			}
		}
	}
}

impl Object for OwnedPageFrames {
	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for OwnedPageFrames {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		f(&self.frames);
	}

	fn physical_pages_len(&self) -> usize {
		self.frames.len()
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(Default::default(), RWX::RWX)
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
