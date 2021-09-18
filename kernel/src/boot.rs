use core::convert::TryFrom;
use core::fmt;

#[repr(C)]
pub struct Info {
	memory_regions_ptr: u32,
	memory_regions_len: u32,
	stack_top: u32,
	stack_bottom: u32,
}

impl Info {
	/// All memory regions that are available and unused.
	///
	/// This *excludes* memory used by the stack and the kernel.
	pub fn memory_regions(&self) -> &[MemoryRegion] {
		unsafe {
			core::slice::from_raw_parts(
				usize::try_from(self.memory_regions_ptr).unwrap() as *const _,
				usize::try_from(self.memory_regions_len).unwrap(),
			)
		}
	}
}

impl fmt::Debug for Info {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Info))
			.field("memory_regions", &self.memory_regions())
			.field("stack", &format_args!("0x{:x} - 0x{:x}", self.stack_bottom, self.stack_top))
			.finish()
	}
}

#[repr(C)]
pub struct MemoryRegion {
	/// Bottom address of the region.
	pub base: u64,
	/// Size of the region in bytes.
	pub size: u64,
}

impl fmt::Debug for MemoryRegion {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "MemoryRegion(0x{:x} - 0x{:x} [0x{:x}])", self.base, self.base + self.size, self.size)
	}
}
