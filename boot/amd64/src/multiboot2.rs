#[repr(C)]
#[repr(align(8))]
struct Header {
	magic: u32,
	flags: u32,
	header_length: u32,
	checksum: u32,
}

#[repr(C)]
#[repr(align(8))]
struct Tag {
	typ: u16,
	flags: u16,
	size: u32,
}

#[repr(C)]
struct FramebufferTag {
	tag: Tag,
	width: u32,
	height: u32,
	depth: u32,
}

#[repr(C)]
struct MultiBoot {
	header: Header,
	framebuffer: FramebufferTag,
	end_tag: Tag,
}

#[link_section = ".multiboot"]
#[used]
static MULTIBOOT: MultiBoot = {
	let magic = 0xE85250D6;
	let flags = 0;
	let header_length = core::mem::size_of::<Header>() as u32;
	let checksum = 0u32
		.wrapping_sub(magic)
		.wrapping_sub(flags)
		.wrapping_sub(header_length);
	MultiBoot {
		header: Header {
			magic,
			flags,
			header_length,
			checksum,
		},
		framebuffer: FramebufferTag {
			tag: Tag {
				typ: 5,
				flags: 0,
				size: core::mem::size_of::<FramebufferTag>() as _,
			},
			width: 0,
			height: 0,
			depth: 32, // RGBX8888 or similar, hopefully
		},
		end_tag: Tag {
			typ: 0,
			flags: 0,
			size: core::mem::size_of::<Tag>() as u32,
		},
	}
};

pub mod bootinfo {

	use core::marker::PhantomData;
	use core::mem;
	use core::slice;

	#[repr(C)]
	#[repr(align(8))]
	struct FixedPart {
		total_size: u32,
		_reserved: u32,
	}

	#[repr(C)]
	#[repr(align(8))]
	struct Tag {
		typ: u32,
		size: u32,
	}

	pub struct Module<'a> {
		pub start: u32,
		pub end: u32,
		pub string: &'a [u8],
	}

	pub struct MemoryMap<'a> {
		pub entries: &'a [MemoryMapEntry],
	}

	#[repr(C)]
	pub struct MemoryMapEntry {
		pub base_address: u64,
		pub length: u64,
		pub typ: u32,
		_reserved: u32,
	}

	impl MemoryMapEntry {
		pub fn is_available(&self) -> bool {
			self.typ == 1
		}
	}

	#[derive(Debug)]
	pub struct FramebufferInfo<'a> {
		pub addr: u64,
		pub pitch: u32,
		pub width: u32,
		pub height: u32,
		pub bpp: u8,
		pub color_info: FramebufferColorInfo<'a>,
	}

	#[derive(Debug)]
	pub enum FramebufferColorInfo<'a> {
		IndexedColor(&'a [FramebufferPaletteEntry]),
		DirectRgbColor(FramebufferDirectRgbColor),
		EgaText,
		Unknown(u8),
	}

	#[derive(Debug)]
	#[repr(C)]
	pub struct FramebufferPaletteEntry {
		pub r: u8,
		pub g: u8,
		pub b: u8,
	}

	#[derive(Debug)]
	pub struct FramebufferDirectRgbColor {
		pub r_pos: u8,
		pub r_mask: u8,
		pub g_pos: u8,
		pub g_mask: u8,
		pub b_pos: u8,
		pub b_mask: u8,
	}

	pub enum Info<'a> {
		Module(Module<'a>),
		MemoryMap(MemoryMap<'a>),
		FramebufferInfo(FramebufferInfo<'a>),
		AcpiRsdp(&'a rsdp::Rsdp),
		Unknown(u32),
	}

	pub struct BootInfo<'a> {
		ptr: *const u8,
		end: *const u8,
		_marker: PhantomData<&'a ()>,
	}

	impl<'a> BootInfo<'a> {
		const MODULE: u32 = 3;
		const MEMORY_MAP: u32 = 6;
		const FRAMEBUFFER_INFO: u32 = 8;
		const ACPI_OLD_RSDP: u32 = 14;
		const ACPI_NEW_RSDP: u32 = 15;

		/// # Safety
		///
		/// The pointer must be properly aligned and point to a valid multiboot2 info structure.
		pub unsafe fn new(ptr: *const u8) -> Self {
			debug_assert_eq!(ptr.align_offset(8), 0);
			let size = usize::try_from((*ptr.cast::<FixedPart>()).total_size).unwrap();
			Self {
				ptr: ptr.add(mem::size_of::<FixedPart>()),
				end: ptr.add(size),
				_marker: PhantomData,
			}
		}
	}

	impl<'a> Iterator for BootInfo<'a> {
		type Item = Info<'a>;

		fn next(&mut self) -> Option<Self::Item> {
			(self.ptr < self.end).then(|| {
				let ptr = self.ptr;
				let tag = unsafe { &*ptr.cast::<Tag>() };
				let size = usize::try_from(tag.size).unwrap();
				self.ptr = ptr.wrapping_add(size);
				self.ptr = self.ptr.wrapping_add(self.ptr.align_offset(8));
				let ptr = ptr.wrapping_add(mem::size_of::<Tag>());
				let size = size - mem::size_of::<Tag>();

				match tag.typ {
					Self::MODULE => {
						debug_assert!(size >= mem::size_of::<u32>() * 2);
						unsafe {
							let start = *ptr.cast::<u32>();
							let end = *ptr.cast::<u32>().wrapping_add(1);
							let string = ptr.wrapping_add(mem::size_of::<u32>() * 2);
							let mut len = 0;
							while *string.add(len) != 0 {
								len += 1;
								debug_assert!(len <= size - mem::size_of::<u32>() * 2);
							}
							let string = slice::from_raw_parts(string, len);
							Info::Module(Module { start, end, string })
						}
					}
					Self::MEMORY_MAP => {
						debug_assert!(
							size >= mem::size_of::<u32>() * 2 + mem::size_of::<MemoryMapEntry>()
						);
						unsafe {
							let entry_size = mem::size_of::<MemoryMapEntry>() as u32;
							let entry_version = 0;
							debug_assert_eq!(*ptr.cast::<u32>(), entry_size);
							debug_assert_eq!(*ptr.cast::<u32>().add(1), entry_version);
							Info::MemoryMap(MemoryMap {
								entries: slice::from_raw_parts(
									ptr.add(mem::size_of::<u32>() * 2).cast(),
									(size - mem::size_of::<u32>() * 2)
										/ mem::size_of::<MemoryMapEntry>(),
								),
							})
						}
					}
					Self::FRAMEBUFFER_INFO => {
						const SIZE: usize = 8 + 4 * 3 + 3;
						debug_assert!(size >= SIZE);
						unsafe {
							Info::FramebufferInfo(FramebufferInfo {
								addr: ptr.cast::<u64>().read(),
								pitch: ptr.add(8).cast::<u32>().read(),
								width: ptr.add(12).cast::<u32>().read(),
								height: ptr.add(16).cast::<u32>().read(),
								bpp: ptr.add(20).read(),
								color_info: match ptr.add(21).read() {
									0 => FramebufferColorInfo::IndexedColor({
										let len = ptr.add(24).cast::<u32>().read();
										slice::from_raw_parts(
											ptr.add(28).cast::<FramebufferPaletteEntry>(),
											len.try_into().unwrap(),
										)
									}),
									1 => FramebufferColorInfo::DirectRgbColor(
										FramebufferDirectRgbColor {
											r_pos: ptr.add(24).read(),
											r_mask: ptr.add(25).read(),
											g_pos: ptr.add(26).read(),
											g_mask: ptr.add(27).read(),
											b_pos: ptr.add(28).read(),
											b_mask: ptr.add(29).read(),
										},
									),
									2 => FramebufferColorInfo::EgaText,
									ty => FramebufferColorInfo::Unknown(ty),
								},
							})
						}
					}
					Self::ACPI_OLD_RSDP => {
						// FIXME the sizes differ. I don't know what to do about it.
						//debug_assert_eq!(size, mem::size_of::<rsdp::Rsdp>());
						unsafe { Info::AcpiRsdp(&*ptr.cast::<rsdp::Rsdp>()) }
					}
					Self::ACPI_NEW_RSDP => {
						debug_assert_eq!(size, mem::size_of::<rsdp::Rsdp>());
						unsafe { Info::AcpiRsdp(&*ptr.cast::<rsdp::Rsdp>()) }
					}
					_ => Info::Unknown(tag.typ),
				}
			})
		}
	}
}
