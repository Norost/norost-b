use core::mem::MaybeUninit;

#[repr(C)]
#[repr(align(8))]
pub struct Info {
	pub memory_regions_offset: u16,
	pub memory_regions_len: u16,
	pub _padding: u32,
	pub memory_top: u64,
	pub initfs_ptr: u32,
	pub initfs_len: u32,
	pub framebuffer: Framebuffer,
	pub rsdp: MaybeUninit<rsdp::Rsdp>,
}

#[repr(C)]
#[repr(align(8))]
pub struct Framebuffer {
	pub base: u64,
	pub pitch: u16,
	pub width: u16,
	pub height: u16,
	pub bpp: u8,
	pub r_pos: u8,
	pub r_mask: u8,
	pub g_pos: u8,
	pub g_mask: u8,
	pub b_pos: u8,
	pub b_mask: u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
#[repr(align(8))]
pub struct MemoryRegion {
	pub base: u64,
	pub size: u64,
}
