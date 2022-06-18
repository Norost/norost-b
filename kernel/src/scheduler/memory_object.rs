use crate::memory::frame::PPN;
use core::any::Any;
use core::ops::Range;

/// Objects which can be mapped into an address space.
///
/// # Safety
///
/// The returned mappings must be valid.
pub unsafe trait MemoryObject
where
	Self: Any,
{
	/// The physical pages used by this object that must be mapped.
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]));

	/// The total amount of physical pages.
	fn physical_pages_len(&self) -> usize;

	/// Mark a range of physical pages as dirty. May panic if the range
	/// is invalid.
	fn mark_dirty(&mut self, range: Range<usize>) {
		let _ = range;
	}
}
