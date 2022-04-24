use core::fmt;

#[repr(C)]
pub struct Info {
	pub memory_regions_offset: u16,
	pub memory_regions_len: u16,
	pub drivers_offset: u16,
	pub drivers_len: u16,
	pub init_offset: u16,
	pub init_len: u16,
	_padding: u32,
	#[cfg(target_arch = "x86_64")]
	pub rsdp: rsdp::Rsdp,
}

impl Info {
	/// All memory regions that are available and unused.
	///
	/// This *excludes* memory used by the stack and the kernel.
	pub fn memory_regions(&self) -> &[MemoryRegion] {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.memory_regions_offset.into());
			core::slice::from_raw_parts(b.cast(), usize::from(self.memory_regions_len))
		}
	}

	/// All drivers to be loaded at boot.
	pub fn drivers(&self) -> &[Driver] {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.drivers_offset.into());
			core::slice::from_raw_parts(b.cast(), usize::from(self.drivers_len))
		}
	}

	/// All init programs & arguments to run.
	pub fn init_programs(&self) -> &[InitProgram] {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.drivers_offset.into());
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
	/// Offset to the name of the driver.
	pub name_offset: u16,
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
			self.size,
		)
	}
}

#[repr(C)]
pub struct InitProgram {
	pub name_offset: u16,
	pub args_offset: u16,
}
