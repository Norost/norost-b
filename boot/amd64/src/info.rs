use core::mem;
use core::mem::MaybeUninit;
use core::ptr;

#[repr(C)]
pub struct Info {
	memory_regions_len: u16,
	drivers_len: u8,
	_padding: [u8; 5],
	rsdp: MaybeUninit<rsdp::Rsdp>,
	_padding2: [u8; 4],
	buffer: [u8; 2048 - 8 - mem::size_of::<rsdp::Rsdp>() - 4],
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MemoryRegion {
	pub base: u64,
	pub size: u64,
}

#[derive(Clone, Copy)]
pub struct Driver {
	pub address: u32,
	pub size: u32,
}

impl Info {
	pub const fn empty() -> Self {
		Self {
			memory_regions_len: 0,
			drivers_len: 0,
			_padding: [0; 5],
			rsdp: MaybeUninit::uninit(),
			_padding2: [0; 4],
			buffer: [0; 2040 - mem::size_of::<rsdp::Rsdp>() - 4],
		}
	}

	pub fn set_rsdp(&mut self, rsdp: &rsdp::Rsdp) {
		self.rsdp.write(*rsdp);
	}

	pub fn set_memory_regions(&mut self, memory_regions: &[MemoryRegion]) {
		assert_eq!(
			self.drivers_len, 0,
			"set_memory_regions must be called before set_drivers"
		);
		self.memory_regions_len = memory_regions.len().try_into().unwrap();
		unsafe {
			let b = self.buffer.as_mut_ptr();
			ptr::copy_nonoverlapping(memory_regions.as_ptr(), b.cast(), memory_regions.len());
		}
	}

	pub fn set_drivers(&mut self, drivers: &[Driver]) {
		self.drivers_len = drivers.len().try_into().unwrap();
		unsafe {
			let b = self
				.buffer
				.as_mut_ptr()
				.cast::<MemoryRegion>()
				.add(usize::from(self.memory_regions_len));
			ptr::copy_nonoverlapping(drivers.as_ptr(), b.cast(), drivers.len());
		}
	}
}
