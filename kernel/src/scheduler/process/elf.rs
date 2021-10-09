use crate::memory::frame;
use crate::memory::frame::PPN;
use crate::memory::r#virtual::{virt_to_phys, MapError, RWX};
use crate::memory::Page;
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

impl ProgramHeader {
	const TYPE_LOAD: u32 = 1;
}

const TYPE_EXEC: u16 = 2;
const MACHINE: u16 = 0x3e;
const FLAGS: u32 = 0;

const FLAG_EXEC: u32 = 0x1;
const FLAG_WRITE: u32 = 0x2;
const FLAG_READ: u32 = 0x4;

impl super::Process {
	pub fn from_elf(data: &[u8]) -> Result<Self, ElfError> {
		let mut slf = Self::new()?;

		(data.len() >= 16)
			.then(|| ())
			.ok_or(ElfError::DataTooShort)?;

		// SAFETY: the data is at least 16 bytes long
		let identifier = unsafe { &*(data as *const [u8] as *const Identifier) };

		(&identifier.magic == b"\x7fELF")
			.then(|| ())
			.ok_or(ElfError::BadMagic)?;
		(data.as_ptr().align_offset(mem::size_of::<usize>()) == 0)
			.then(|| ())
			.ok_or(ElfError::BadAlignment)?;

		const ID_ELF64: u8 = 2;
		const LITTLE_ENDIAN: u8 = 1;
		(identifier.class == ID_ELF64)
			.then(|| ())
			.ok_or(ElfError::UnsupportedClass)?;
		(identifier.data == LITTLE_ENDIAN)
			.then(|| ())
			.ok_or(ElfError::UnsupportedEndian)?;
		(identifier.version == 1)
			.then(|| ())
			.ok_or(ElfError::UnsupportedVersion)?;

		(data.len() >= mem::size_of::<FileHeader>())
			.then(|| ())
			.ok_or(ElfError::DataTooShort)?;
		// SAFETY: the data is long enough
		let header = unsafe { &*(data as *const [u8] as *const FileHeader) };

		(header.typ == TYPE_EXEC)
			.then(|| ())
			.ok_or(ElfError::UnsupportedType(header.typ))?;
		(header.machine == MACHINE)
			.then(|| ())
			.ok_or(ElfError::UnsupportedMachine)?;
		(header.flags & !FLAGS == 0)
			.then(|| ())
			.ok_or(ElfError::UnsupportedFlags)?;

		// Parse the program headers and create the segments.

		let count = header.program_header_entry_count as usize;
		let size = header.program_header_entry_size as usize;

		(size == mem::size_of::<ProgramHeader>())
			.then(|| ())
			.ok_or(ElfError::ProgramHeaderSizeMismatch)?;
		let h_offt = usize::try_from(header.program_header_offset)
			.map_err(|_| ElfError::OffsetOutOfBounds)?;
		(data.len() >= count * size + h_offt)
			.then(|| ())
			.ok_or(ElfError::OffsetOutOfBounds)?;

		for k in 0..count {
			// SAFETY: the data is large enough and aligned and the header size matches.
			let header = unsafe {
				let h = data as *const [u8] as *const u8;
				let h = h.add(
					header
						.program_header_offset
						.try_into()
						.map_err(|_| ElfError::OffsetOutOfBounds)?,
				);
				let h = h as *const ProgramHeader;
				&*h.add(k)
			};

			// Skip non-loadable segments
			if header.typ != ProgramHeader::TYPE_LOAD {
				continue;
			}

			let f = header.flags;

			let page_mask = u64::try_from(Page::MASK).unwrap();
			let page_size = u64::try_from(Page::SIZE).unwrap();

			(header.offset & page_mask == header.virtual_address & page_mask)
				.then(|| ())
				.ok_or(ElfError::AddressOffsetMismatch)?;

			let offset = header.offset & page_mask;

			let virt_address = header.virtual_address & !page_mask;
			let phys_address =
				(unsafe { virt_to_phys(data.as_ptr()) } + header.offset) & !page_mask;
			let count = (header.file_size + page_mask) / page_size;
			let rwx = RWX::from_flags(f & FLAG_READ > 0, f & FLAG_WRITE > 0, f & FLAG_EXEC > 0)?;
			for i in 0..count {
				let virt = (virt_address + i * page_size) as *const _;
				let phys =
					PPN::try_from_usize(usize::try_from(phys_address + i * page_size).unwrap())
						.unwrap();
				unsafe {
					slf.address_space
						.map(virt, [phys].iter().copied(), rwx, slf.hint_color)?;
				}
			}
			let alloc = (header.memory_size + offset + page_mask) / page_size;
			for i in count..alloc {
				let virt = (virt_address + i * page_size) as *const _;
				let phys = frame::allocate_contiguous(1)?;
				unsafe {
					slf.address_space
						.map(virt, [phys].iter().copied(), rwx, slf.hint_color)?;
				}
			}
		}

		slf.thread = Some(super::super::Thread::new(header.entry.try_into().unwrap())?);

		Ok(slf)
	}
}

#[derive(Debug)]
pub enum ElfError {
	DataTooShort,
	BadMagic,
	BadAlignment,
	UnsupportedClass,
	UnsupportedEndian,
	UnsupportedVersion,
	UnsupportedType(u16),
	UnsupportedMachine,
	UnsupportedFlags,
	IncompatibleRWXFlags,
	ProgramHeaderSizeMismatch,
	OffsetOutOfBounds,
	AddressOffsetMismatch,
	AllocateError(frame::AllocateError),
	AllocateContiguousError(frame::AllocateContiguousError),
	MapError(MapError),
}

impl From<frame::AllocateContiguousError> for ElfError {
	fn from(err: frame::AllocateContiguousError) -> Self {
		Self::AllocateContiguousError(err)
	}
}

impl From<crate::memory::r#virtual::IncompatibleRWXFlags> for ElfError {
	fn from(_: crate::memory::r#virtual::IncompatibleRWXFlags) -> Self {
		Self::IncompatibleRWXFlags
	}
}

impl From<MapError> for ElfError {
	fn from(err: MapError) -> Self {
		Self::MapError(err)
	}
}
