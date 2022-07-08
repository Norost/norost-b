pub mod frame;
mod heap;
pub mod r#virtual;

use crate::{boot, object_table::Root};

#[cfg(target_arch = "x86_64")]
#[repr(align(4096))]
pub struct Page([u8; Self::SIZE]);

impl Page {
	pub const SIZE: usize = 4096;
	pub const MASK: usize = 0xfff;
	pub const OFFSET_BITS: u8 = 12;
	#[deprecated]
	pub const OFFSET_MASK: usize = 0xfff;

	pub const fn min_pages_for_bytes(bytes: usize) -> usize {
		(bytes + Self::SIZE - 1) / Self::SIZE
	}
}

/// # Safety
///
/// This may only be called once at boot time.
pub(super) unsafe fn init(memory_regions: &mut [boot::MemoryRegion]) {
	unsafe { frame::init(memory_regions) }
}

pub(super) fn post_init(root: &Root) {
	frame::post_init(root)
}
