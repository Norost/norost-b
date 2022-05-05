use crate::util::{DebugByteStr, DebugIter};
use core::fmt;

#[repr(C)]
pub struct Info {
	pub memory_regions_offset: u16,
	pub memory_regions_len: u16,
	pub drivers_offset: u16,
	pub drivers_len: u16,
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
	pub fn drivers(&self) -> impl Iterator<Item = Driver<'_>> {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.drivers_offset.into());
			core::slice::from_raw_parts(b.cast(), usize::from(self.drivers_len))
				.iter()
				.map(|inner: &RawDriver| Driver { info: self, inner })
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
			.field("drivers", &DebugIter::new(self.drivers()))
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

#[repr(C)]
struct RawDriver {
	/// Start address of the ELF file.
	pub address: u32,
	/// Size of the ELF file in bytes.
	pub size: u32,
	/// Offset to the name of the driver.
	pub name_offset: u16,
}

/// A driver to load at boot.
pub struct Driver<'a> {
	info: &'a Info,
	inner: &'a RawDriver,
}

impl<'a> Driver<'a> {
	/// Return the raw binary contents of the driver.
	///
	/// # Safety
	///
	/// It is up to the caller to ensure the region is still accessible.
	pub unsafe fn as_slice<'b>(&self) -> &'b [u8] {
		unsafe {
			let a = crate::memory::r#virtual::phys_to_virt(self.inner.address.into());
			core::slice::from_raw_parts(a, self.inner.size.try_into().unwrap())
		}
	}

	/// The name of the driver.
	pub fn name(&self) -> &'a [u8] {
		self.info.get_str(self.inner.name_offset)
	}
}

impl fmt::Debug for Driver<'_> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Driver))
			.field("name", &DebugByteStr::new(self.name()))
			.field(
				"range",
				&format_args!(
					"{:#x}..{:#x}",
					self.inner.address,
					self.inner.address + self.inner.size
				),
			)
			.finish()
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
