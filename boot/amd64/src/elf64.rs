use crate::paging::{Page, PML4};
use core::convert::{TryFrom, TryInto};
use core::mem;

#[repr(C)]
struct FileHeader {
	identifier: Identifier,

	typ: u16,

	machine: u16,

	version: u32,

	entry: u64,
	program_header_offset: u64,
	section_header_offset: u64,

	flags: u32,
	header_size: u16,

	program_header_entry_size: u16,

	program_header_entry_count: u16,
	section_header_entry_size: u16,
	section_header_entry_count: u16,
	section_header_str_rndx: u16,
}
const _FILE_HEADER_SIZE_CHECK: usize = 0 - (64 - mem::size_of::<FileHeader>());

#[repr(C)]
struct Identifier {
	magic: [u8; 4],
	class: u8,
	data: u8,
	version: u8,
	_padding: [u8; 9],
}
const _IDENTIFIER_SIZE_CHECK: usize = 0 - (16 - mem::size_of::<Identifier>());

#[repr(C)]
struct ProgramHeader {
	typ: u32,
	flags: u32,
	offset: u64,
	virtual_address: u64,
	physical_address: u64,
	file_size: u64,
	memory_size: u64,
	alignment: u64,
}
const _PROGRAM_HEADER_SIZE_CHECK: usize = 0 - (56 - mem::size_of::<ProgramHeader>());

const TYPE_EXEC: u16 = 2;
const MACHINE: u16 = 0x3e;
const FLAGS: u32 = 0;

pub fn load_elf<F>(data: &[u8], mut page_alloc: F, page_tables: &mut PML4) -> u64
where
	F: FnMut() -> *mut Page,
{
	assert!(data.len() >= 16, "Data too short to include magic");

	// SAFETY: the data is at least 16 bytes long
	let identifier = unsafe { &*(data as *const [u8] as *const Identifier) };

	assert_eq!(&identifier.magic, b"\x7fELF", "Bad ELF magic");
	assert_eq!(
		data.as_ptr().align_offset(mem::size_of::<usize>()),
		0,
		"Bad alignment"
	);

	const ID_ELF64: u8 = 2;
	assert_eq!(identifier.class, ID_ELF64, "Unsupported class");

	#[cfg(target_endian = "little")]
	assert_eq!(identifier.data, 1, "Unsupported endianness");
	#[cfg(target_endian = "big")]
	assert_eq!(identifier.data, 2, "Unsupported endianness");

	assert_eq!(identifier.version, 1, "Unsupported version");

	assert!(
		data.len() >= mem::size_of::<FileHeader>(),
		"Header too small"
	);
	// SAFETY: the data is long enough
	let header = unsafe { &*(data as *const [u8] as *const FileHeader) };

	assert_eq!(header.typ, TYPE_EXEC, "Unsupported type");

	assert_eq!(header.machine, MACHINE, "Unsupported machine type");

	assert_eq!(header.flags & !FLAGS, 0, "Unsupported flags");

	// Parse the program headers and create the segments.

	let count = header.program_header_entry_count as usize;
	let size = header.program_header_entry_size as usize;
	assert_eq!(
		size,
		mem::size_of::<ProgramHeader>(),
		"Bad program header size"
	);
	assert!(
		data.len() >= count * size + usize::try_from(header.program_header_offset).unwrap(),
		"Program headers exceed the size of the file"
	);

	for k in 0..count {
		// SAFETY: the data is large enough and aligned and the header size matches.
		let header = unsafe {
			let h = data as *const [u8] as *const u8;
			let h = h.add(header.program_header_offset.try_into().unwrap());
			let h = h as *const ProgramHeader;
			&*h.add(k)
		};

		// Skip non-loadable segments
		if header.typ != ProgramHeader::TYPE_LOAD {
			continue;
		}

		let f = header.flags;

		const PAGE_MASK: u64 = 0xfff;
		const PAGE_SIZE: u64 = 0x1000;

		assert_eq!(
			header.offset & PAGE_MASK,
			header.virtual_address & PAGE_MASK,
			"Offset is not aligned"
		);

		let offset = header.offset & PAGE_MASK;

		let virt_address = header.virtual_address & !PAGE_MASK;
		let phys_address = data.as_ptr() as u64 + header.offset;
		let count = (header.memory_size + offset + PAGE_MASK) / PAGE_SIZE;
		for i in 0..count {
			let virt = virt_address + i * PAGE_SIZE;
			let phys = phys_address + i * PAGE_SIZE;
			let (r, w, x) = (f & FLAG_READ > 0, f & FLAG_WRITE > 0, f & FLAG_EXEC > 0);
			page_tables.add(virt, phys, r, w, x, &mut page_alloc);
		}
	}

	header.entry
}

const FLAG_EXEC: u32 = 0x1;
const FLAG_WRITE: u32 = 0x2;
const FLAG_READ: u32 = 0x4;

impl ProgramHeader {
	const TYPE_LOAD: u32 = 1;
}
