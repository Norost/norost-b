use crate::memory::frame::{PageFrame, PPN};
use alloc::boxed::Box;
use core::ops::Range;

/// Objects which can be mapped into an address space.
pub trait MemoryObject {
	/// The physical pages used by this object that must be mapped.
	fn physical_pages(&self) -> Box<[PageFrame]>;

	/// Mark a range of physical pages as dirty. May panic if the range
	/// is invalid.
	fn mark_dirty(&mut self, range: Range<usize>);
}
