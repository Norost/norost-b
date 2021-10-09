use crate::paging::{AddError, Page, PML4};
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

#[derive(Clone, Copy)]
pub enum ParseError {
	DataTooShort,
	BadMagic,
	BadAlignment,
	UnsupportedClass,
	UnsupportedEndian,
	UnsupportedVersion,
	UnsupportedType,
	UnsupportedMachine,
	UnsupportedFlags,
	ProgramHeaderSizeMismatch,
	OffsetOutOfBounds,
	AddressOffsetMismatch,
	PageAddError(AddError),
}

impl From<ParseError> for &'static str {
	fn from(err: ParseError) -> &'static str {
		match err {
			ParseError::DataTooShort => "data too short",
			ParseError::BadMagic => "bad magic",
			ParseError::BadAlignment => "bad alignment",
			ParseError::UnsupportedClass => "unsupported class",
			ParseError::UnsupportedEndian => "unsupported endian",
			ParseError::UnsupportedVersion => "unsupported version",
			ParseError::UnsupportedType => "unsupported type",
			ParseError::UnsupportedMachine => "unsupported machine",
			ParseError::UnsupportedFlags => "unsupported flags",
			ParseError::ProgramHeaderSizeMismatch => "program header size mismatch",
			ParseError::OffsetOutOfBounds => "offset out of bounds",
			ParseError::AddressOffsetMismatch => "address offset mismatch",
			ParseError::PageAddError(e) => <&'static str as From<_>>::from(e),
		}
	}
}

impl From<AddError> for ParseError {
	fn from(err: AddError) -> Self {
		Self::PageAddError(err)
	}
}

pub fn load_elf<F>(
	data: &[u8],
	mut page_alloc: F,
	page_tables: &mut PML4,
) -> Result<u64, ParseError>
where
	F: FnMut() -> *mut Page,
{
	(data.len() >= 16)
		.then(|| ())
		.ok_or(ParseError::DataTooShort)?;

	// SAFETY: the data is at least 16 bytes long
	let identifier = unsafe { &*(data as *const [u8] as *const Identifier) };

	(&identifier.magic == b"\x7fELF")
		.then(|| ())
		.ok_or(ParseError::BadMagic)?;
	(data.as_ptr().align_offset(mem::size_of::<usize>()) == 0)
		.then(|| ())
		.ok_or(ParseError::BadAlignment)?;

	const ID_ELF64: u8 = 2;
	const LITTLE_ENDIAN: u8 = 1;
	(identifier.class == ID_ELF64)
		.then(|| ())
		.ok_or(ParseError::UnsupportedClass)?;
	(identifier.data == LITTLE_ENDIAN)
		.then(|| ())
		.ok_or(ParseError::UnsupportedEndian)?;
	(identifier.version == 1)
		.then(|| ())
		.ok_or(ParseError::UnsupportedVersion)?;

	(data.len() >= mem::size_of::<FileHeader>())
		.then(|| ())
		.ok_or(ParseError::DataTooShort)?;
	// SAFETY: the data is long enough
	let header = unsafe { &*(data as *const [u8] as *const FileHeader) };

	(header.typ == TYPE_EXEC)
		.then(|| ())
		.ok_or(ParseError::UnsupportedType)?;
	(header.machine == MACHINE)
		.then(|| ())
		.ok_or(ParseError::UnsupportedMachine)?;
	(header.flags & !FLAGS == 0)
		.then(|| ())
		.ok_or(ParseError::UnsupportedFlags)?;

	// Parse the program headers and create the segments.

	let count = header.program_header_entry_count as usize;
	let size = header.program_header_entry_size as usize;

	(size == mem::size_of::<ProgramHeader>())
		.then(|| ())
		.ok_or(ParseError::ProgramHeaderSizeMismatch)?;
	let h_offt =
		usize::try_from(header.program_header_offset).map_err(|_| ParseError::OffsetOutOfBounds)?;
	(data.len() >= count * size + h_offt)
		.then(|| ())
		.ok_or(ParseError::OffsetOutOfBounds)?;

	for k in 0..count {
		// SAFETY: the data is large enough and aligned and the header size matches.
		let header = unsafe {
			let h = data as *const [u8] as *const u8;
			let h = h.add(
				header
					.program_header_offset
					.try_into()
					.map_err(|_| ParseError::OffsetOutOfBounds)?,
			);
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

		(header.offset & PAGE_MASK == header.virtual_address & PAGE_MASK)
			.then(|| ())
			.ok_or(ParseError::AddressOffsetMismatch)?;

		let offset = header.offset & PAGE_MASK;

		let virt_address = header.virtual_address & !PAGE_MASK;
		let phys_address = data.as_ptr() as u64 + header.offset;
		let count = (header.file_size + offset + PAGE_MASK) / PAGE_SIZE;
		for i in 0..count {
			let virt = virt_address + i * PAGE_SIZE;
			let phys = phys_address + i * PAGE_SIZE;
			let (r, w, x) = (f & FLAG_READ > 0, f & FLAG_WRITE > 0, f & FLAG_EXEC > 0);
			page_tables.add(virt, phys, r, w, x, &mut page_alloc)?;
		}
		let alloc = (header.memory_size + offset + PAGE_MASK) / PAGE_SIZE;
		for i in count..alloc {
			let virt = virt_address + i * PAGE_SIZE;
			let phys = page_alloc() as u64;
			let (r, w, x) = (f & FLAG_READ > 0, f & FLAG_WRITE > 0, f & FLAG_EXEC > 0);
			page_tables.add(virt, phys, r, w, x, &mut page_alloc)?;
		}
	}

	Ok(header.entry)
}

const FLAG_EXEC: u32 = 0x1;
const FLAG_WRITE: u32 = 0x2;
const FLAG_READ: u32 = 0x4;

impl ProgramHeader {
	const TYPE_LOAD: u32 = 1;
}
