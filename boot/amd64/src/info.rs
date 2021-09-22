use core::convert::TryInto;
use core::mem::MaybeUninit;

#[repr(C)]
pub struct Info {
	memory_regions_len: u16,
	_padding: [u16; 3],
	memory_regions: [MaybeUninit<MemoryRegion>; 128],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
	pub base: u64,
	pub size: u64,
}

impl Info {
	pub const fn empty() -> Self {
		Self {
			memory_regions_len: 0,
			_padding: [0; 3],
			memory_regions: MaybeUninit::uninit_array(),
		}
	}

	pub fn set_memory_regions(&mut self, memory_regions: &[MemoryRegion]) {
		self.memory_regions.iter_mut().zip(memory_regions.iter()).for_each(|(w, r)| { w.write(*r); });
		self.memory_regions_len = memory_regions.len().try_into().unwrap();
	}
}
