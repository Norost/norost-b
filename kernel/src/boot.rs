use core::fmt;

#[repr(C)]
pub struct Info {
	memory_regions_len: u16,
	drivers_len: u8,
	_padding: [u8; 5],
	#[cfg(target_arch = "x86_64")]
	pub rsdp: rsdp::Rsdp,
	_padding2: [u8; 4],
	memory_regions: [MemoryRegion; 0],
}

impl Info {
	/// All memory regions that are available and unused.
	///
	/// This *excludes* memory used by the stack and the kernel.
	pub fn memory_regions(&self) -> &[MemoryRegion] {
		unsafe {
			let b = self.memory_regions.as_ptr();
			core::slice::from_raw_parts(b, usize::from(self.memory_regions_len))
		}
	}

	/// All drivers to be loaded at boot.
	pub fn drivers(&self) -> &[Driver] {
		unsafe {
			let b = self
				.memory_regions
				.as_ptr()
				.add(usize::from(self.memory_regions_len));
			core::slice::from_raw_parts(b.cast(), usize::from(self.drivers_len))
		}
	}
}

impl fmt::Debug for Info {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Info))
			.field("memory_regions", &self.memory_regions())
			.field("drivers", &self.drivers())
			.field("rsdp", &self.rsdp)
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
		write!(
			f,
			"MemoryRegion(0x{:x} - 0x{:x} [0x{:x}])",
			self.base,
			self.base + self.size,
			self.size
		)
	}
}

#[derive(Clone)]
#[repr(C)]
pub struct Driver {
	/// Start address of the ELF file.
	pub address: u32,
	/// Size of the ELF file in bytes.
	pub size: u32,
}

impl Driver {
	pub fn as_slice(&self) -> &[u8] {
		unsafe {
			let a = crate::memory::r#virtual::phys_to_virt(self.address.into());
			core::slice::from_raw_parts(a, self.size.try_into().unwrap())
		}
	}
}

impl fmt::Debug for Driver {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(
			f,
			"Driver(0x{:x} - 0x{:x} [0x{:x}])",
			self.address,
			self.address + self.size,
			self.size
		)
	}
}
