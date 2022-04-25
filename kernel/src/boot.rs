use crate::util::{DebugByteStr, DebugIter};
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
	pub fn init_programs(&self) -> impl Iterator<Item = InitProgram<'_>> {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.init_offset.into());
			core::slice::from_raw_parts(b.cast(), usize::from(self.init_len))
				.iter()
				.map(|inner: &RawInitProgram| InitProgram { info: self, inner })
		}
	}

	/// Get a byte string from the buffer.
	///
	/// A byte string is prefixed with a single byte that indicates its length.
	fn get_str(&self, offset: u16) -> &[u8] {
		let offset = usize::from(offset);
		let len = self.buffer()[offset];
		&self.buffer()[1 + offset..1 + offset + usize::from(len)]
	}

	/// Cast to an array.
	fn buffer(&self) -> &[u8; 1 << 16] {
		// SAFETY: The boot loader should have given us a sufficiently large buffer.
		unsafe { &*(self as *const Self).cast() }
	}
}

impl fmt::Debug for Info {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Info))
			.field("memory_regions", &self.memory_regions())
			.field("drivers", &self.drivers())
			.field("init_programs", &DebugIter::new(self.init_programs()))
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
struct RawInitProgram {
	driver: u16,
	args_offset: u16,
	args_len: u16,
}

/// A program to run at boot.
pub struct InitProgram<'a> {
	info: &'a Info,
	inner: &'a RawInitProgram,
}

impl<'a> InitProgram<'a> {
	/// The index of the corresponding driver
	pub fn driver(&self) -> u16 {
		self.inner.driver
	}

	/// The arguments that should be passed to this program.
	pub fn args(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
		let mut offset = self.inner.args_offset;
		(0..self.inner.args_len).map(move |_| {
			let s = self.info.get_str(offset);
			offset += 1 + u16::try_from(s.len()).unwrap();
			s
		})
	}
}

impl fmt::Debug for InitProgram<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(stringify!(InitProgram))
			.field("driver", &self.driver())
			.field("args", &DebugIter::new(self.args().map(DebugByteStr::new)))
			.finish()
	}
}
