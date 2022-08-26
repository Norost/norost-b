use crate::memory::{frame::PPN, r#virtual::RWX};
use core::any::Any;

pub use crate::arch::PageFlags;

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
	///
	/// If the closure returns `false`, this function **must** stop calling the closure.
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool);

	/// The total amount of physical pages.
	fn physical_pages_len(&self) -> usize;

	/// The RWX permissions and other architecture-specific flags that may be used for these pages.
	fn page_flags(&self) -> (PageFlags, RWX);
}
