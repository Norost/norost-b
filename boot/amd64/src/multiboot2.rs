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
struct MultiBoot {
	header: Header,
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
		end_tag: Tag {
			typ: 0,
			flags: 0,
			size: core::mem::size_of::<Tag>() as u32,
		},
	}
};

pub mod bootinfo {

	use core::convert::TryFrom;
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

	pub enum Info<'a> {
		Module(Module<'a>),
		MemoryMap(MemoryMap<'a>),
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
					_ => Info::Unknown(tag.typ),
				}
			})
		}
	}
}
