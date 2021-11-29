pub mod frame;
mod heap;
pub mod r#virtual;

#[cfg(target_arch = "x86_64")]
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
