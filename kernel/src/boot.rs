use crate::memory::Page;
use core::fmt;

#[repr(C)]
pub struct Info {
	pub memory_regions_offset: u16,
	pub memory_regions_len: u16,
	pub vsyscall_phys_addr: u32,
	pub memory_top: u64,
	pub initfs_ptr: u32,
	pub initfs_len: u32,
	pub framebuffer: Framebuffer,
	#[cfg(target_arch = "x86_64")]
	pub rsdp: rsdp::Rsdp,
}

impl Info {
	/// All memory regions that are available and unused.
	///
	/// This *excludes* memory used by the stack and the kernel.
	pub fn memory_regions_mut(&mut self) -> &mut [MemoryRegion] {
		unsafe {
			let b = (self as *mut _ as *mut u8).add(self.memory_regions_offset.into());
			core::slice::from_raw_parts_mut(b.cast(), usize::from(self.memory_regions_len))
		}
	}

	/// All memory regions that are available and unused.
	///
	/// This *excludes* memory used by the stack and the kernel.
	pub fn memory_regions(&self) -> &[MemoryRegion] {
		unsafe {
			let b = (self as *const _ as *const u8).add(self.memory_regions_offset.into());
			core::slice::from_raw_parts(b.cast(), usize::from(self.memory_regions_len))
		}
	}
}

impl fmt::Debug for Info {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Info))
			.field("memory_regions", &self.memory_regions())
			.field(
				"initfs",
				&format_args!(
					"{:#x} - {:#x}",
					self.initfs_ptr,
					self.initfs_ptr + self.initfs_len - 1
				),
			)
			.field("rsdp", &self.rsdp)
			.finish()
	}
}

#[repr(C)]
pub struct MemoryRegion {
	/// Bottom address of the region.
	base: u64,
	/// Size of the region in bytes.
	size: u64,
}

impl MemoryRegion {
	/// The total amount of bytes.
	pub fn size(&self) -> u64 {
		self.size
	}

	/// Take a single page from the memory region.
	pub fn take_page(&mut self) -> Option<u64> {
		let page_size = Page::SIZE.try_into().unwrap();
		self.size.checked_sub(page_size).map(|s| {
			let base = self.base;
			self.base += page_size;
			self.size = s;
			debug_assert_eq!(base & 0xfff, 0);
			base
		})
	}

	/// Take a range of pages from the memory region.
	pub fn take_page_range(&mut self, count: usize) -> Option<u64> {
		let page_size = u64::try_from(Page::SIZE).unwrap();
		let count = u64::try_from(count).unwrap();
		self.size.checked_sub(page_size * count).map(|s| {
			let base = self.base;
			self.base += page_size;
			self.size = s;
			debug_assert_eq!(base & 0xfff, 0);
			base
		})
	}
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

#[repr(C, align(8))]
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
