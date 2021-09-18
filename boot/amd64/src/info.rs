use core::convert::TryInto;
use core::marker::PhantomData;

#[repr(C)]
pub struct Info<'a> {
	memory_regions_ptr: u32,
	memory_regions_len: u32,
	stack_top: u32,
	stack_bottom: u32,
	_marker: PhantomData<&'a ()>,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
	pub base: u64,
	pub size: u64,
}

impl<'a> Info<'a> {
	pub fn new(memory_regions: &'a [MemoryRegion], stack: (usize, usize)) -> Self {
		Self {
			memory_regions_ptr: (memory_regions.as_ptr() as usize).try_into().unwrap(),
			memory_regions_len: memory_regions.len().try_into().unwrap(),
			stack_top: stack.0.try_into().unwrap(),
			stack_bottom: stack.1.try_into().unwrap(),
			_marker: PhantomData,
		}
	}
}
