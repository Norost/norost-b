use crate::memory::frame::{self, AllocateHints, OwnedPageFrames};
use crate::memory::r#virtual::{MapError, RWX};
use crate::memory::Page;
use crate::object_table::Object;
use crate::scheduler::{process::frame::PPN, MemoryObject};
use alloc::{boxed::Box, sync::Arc};
use core::mem;
use core::num::NonZeroUsize;
use core::ops::Range;
use core::ptr::NonNull;

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

struct MemorySlice {
	inner: Arc<dyn MemoryObject>,
	range: Range<usize>,
}

impl MemoryObject for MemorySlice {
	fn physical_pages(&self) -> Box<[PPN]> {
		self.inner
			.physical_pages()
			.into_vec()
			.into_iter()
			.skip(self.range.start)
			.take(self.range.end - self.range.start)
			.collect()
	}
}

impl super::Process {
	pub fn from_elf(
		data_object: Arc<dyn MemoryObject>,
		stack_frames: Option<OwnedPageFrames>,
		stack_offset: usize,
		objects: arena::Arena<Arc<dyn Object>, u8>,
	) -> Result<Arc<Self>, ElfError> {
		// FIXME don't require contiguous pages.
		let data = data_object.physical_pages();

		// FIXME definitely don't require unsafe code.
		let data = unsafe {
			core::slice::from_raw_parts(data[0].as_ptr().cast::<u8>(), Page::SIZE * data.len())
		};

		let slf = Self::new()?;
		*slf.objects.lock() = objects;

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

		let mut address_space = slf.address_space.lock();

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

			(header.offset & page_mask == header.virtual_address & page_mask)
				.then(|| ())
				.ok_or(ElfError::AddressOffsetMismatch)?;

			let page_offset = usize::try_from(header.offset >> Page::OFFSET_BITS).unwrap();
			let (phys, virt) = (header.physical_address, header.virtual_address);
			let count = page_count(phys..phys + header.file_size);
			let alloc = page_count(virt..virt + header.memory_size);

			let virt_address = header.virtual_address & !page_mask;
			let virt_address = usize::try_from(virt_address).unwrap();
			let rwx = RWX::from_flags(f & FLAG_READ > 0, f & FLAG_WRITE > 0, f & FLAG_EXEC > 0)?;

			if rwx.w() {
				if let Some(alloc) = NonZeroUsize::new(alloc) {
					// Allocate & copy
					let virt = NonNull::new(virt_address as *mut _).unwrap();
					let hint = AllocateHints {
						address: virt.cast().as_ptr(),
						color: slf.hint_color,
					};
					let mem = Arc::new(
						OwnedPageFrames::new(alloc, hint).map_err(ElfError::AllocateError)?,
					);
					// FIXME this is utter shit
					let mut offt = 0;
					let page_offt = usize::try_from(header.offset).unwrap() & Page::MASK;
					let mut wr_i = page_offt;
					for p in data_object.physical_pages().into_vec() {
						let (from, to) = (offt, offt + u64::try_from(Page::SIZE).unwrap());
						for i in from.max(header.offset)..to.min(header.offset + header.file_size) {
							let rd_i = usize::try_from(i).unwrap() & Page::MASK;
							assert_eq!(rd_i & Page::MASK, wr_i & Page::MASK);
							unsafe {
								let b = p.as_ptr().cast::<u8>().add(rd_i).read();
								mem.write(wr_i, &[b]);
							}
							wr_i += 1;
						}
						offt = to;
					}
					assert_eq!(u64::try_from(wr_i - page_offt).unwrap(), header.file_size);
					address_space
						.map_object(Some(virt), mem, rwx, slf.hint_color)
						.map_err(ElfError::MapError)?;
				}
			} else {
				if let Some(count) = NonZeroUsize::new(count) {
					// Map part of the ELF file.
					let virt = NonNull::new(virt_address as *mut _).unwrap();
					let mem = Arc::new(MemorySlice {
						inner: data_object.clone(),
						range: page_offset..page_offset + count.get(),
					});
					address_space
						.map_object(Some(virt), mem, rwx, slf.hint_color)
						.map_err(ElfError::MapError)?;
				}
				// Allocate memory for the region that isn't present in the ELF file.
				if let Some(size) = NonZeroUsize::new(alloc - count) {
					let virt = NonNull::new((virt_address + count * Page::SIZE) as *mut _).unwrap();
					let hint = AllocateHints {
						address: virt.cast().as_ptr(),
						color: slf.hint_color,
					};
					let mem = Arc::new(
						OwnedPageFrames::new(size, hint).map_err(ElfError::AllocateError)?,
					);
					address_space
						.map_object(Some(virt), mem, rwx, slf.hint_color)
						.map_err(ElfError::MapError)?;
				}
			}
		}

		// Map in stack
		let stack = if let Some(stack_frames) = stack_frames {
			let stack = address_space
				.map_object(None, Arc::new(stack_frames), RWX::RW, slf.hint_color)
				.map_err(ElfError::MapError)?;
			stack.as_ptr().wrapping_add(stack_offset) as usize
		} else {
			0
		};

		drop(address_space);

		let slf = Arc::new(slf);

		slf.spawn_thread(header.entry.try_into().unwrap(), stack)
			.unwrap();

		Ok(slf)
	}
}

/// Determine the amount of pages needed to cover an address range
fn page_count(range: Range<u64>) -> usize {
	let (pm, ps) = (
		u64::try_from(Page::MASK).unwrap(),
		u64::try_from(Page::SIZE).unwrap(),
	);
	let start = range.start & !pm;
	let end = range.end.wrapping_add(pm) & !pm;
	(end.wrapping_sub(start) / ps).try_into().unwrap()
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
