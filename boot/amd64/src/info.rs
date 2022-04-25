use core::mem::MaybeUninit;

#[repr(C)]
#[repr(align(8))]
pub struct Info {
	pub memory_regions_offset: u16,
	pub memory_regions_len: u16,
	pub drivers_offset: u16,
	pub drivers_len: u16,
	pub init_offset: u16,
	pub init_len: u16,
	// Ensure rsdp has 64 bit alignment.
	pub _padding: u32,
	pub rsdp: MaybeUninit<rsdp::Rsdp>,
}

#[derive(Clone, Copy)]
#[repr(C)]
#[repr(align(8))]
pub struct MemoryRegion {
	pub base: u64,
	pub size: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Driver {
	pub address: u32,
	pub size: u32,
	pub name_offset: u16,
	pub _padding: u16,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct InitProgram {
	pub driver: u16,
	pub args_offset: u16,
	pub args_len: u16,
}
