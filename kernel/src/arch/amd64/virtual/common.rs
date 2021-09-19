use crate::memory::frame;
use crate::memory::Page;
use core::convert::{TryFrom, TryInto};
use core::mem;
use core::ptr;

pub struct Entry(u64);

pub const IDEMPOTENT_MAP_ADDRESS: *mut u8 = 0xffff_c000_0000_0000 as *mut _;

impl Entry {
	const PRESENT: u64 = 1 << 0;
	const READ_WRITE: u64 = 1 << 1;
	const USER: u64 = 1 << 2;
	const WRITE_THROUGH: u64 = 1 << 3;
	const CACHE_DISABLE: u64 = 1 << 4;
	const ACCESSED: u64 = 1 << 5;
	const PAGE_SIZE: u64 = 1 << 7;
	const GLOBAL: u64 = 1 << 8;
	const AVAILABLE: u64 = 7 << 9;

	pub fn is_present(&self) -> bool {
		self.0 & Self::PRESENT > 0
	}

	pub fn is_table(&self) -> bool {
		self.is_present() && !self.is_leaf()
	}

	pub fn is_leaf(&self) -> bool {
		self.is_present() && self.0 & Self::PAGE_SIZE > 0
	}

	pub fn as_table_mut(&mut self) -> Option<&mut [Entry; 512]> {
		// SAFETY: FIXME not sure how to guarantee safety :/
		self.is_table().then(|| unsafe {
			&mut *phys_to_virt(self.0 & !u64::try_from(Page::OFFSET_MASK).unwrap()).cast()
		})
	}

	pub fn make_table(&mut self, hint_color: u8) -> Result<&mut [Entry; 512], MakeTableError> {
		if self.is_table() {
			// The borrow checked is retarded, so this will have to do.
			Ok(self.as_table_mut().unwrap())
		} else if self.is_leaf() {
			Err(MakeTableError::IsMapped)
		} else {
			let mut frame = None;
			frame::allocate(1, |f| frame = Some(f), self as *mut _ as *mut _, hint_color)?;
			Ok(self.new_table(frame.unwrap()))
		}
	}

	pub fn new_table(&mut self, frame: frame::PageFrame) -> &mut [Entry; 512] {
		assert_eq!(frame.p2size, 0);
		let frame = frame.base.try_into().unwrap();
		// SAFETY: FIXME the allocator makes no guarantees about the address of the frame.
		let tbl = unsafe { phys_to_virt(frame).cast::<[Entry; 512]>() };
		// SAFETY: a fully zeroed Entry is valid.
		unsafe { ptr::write_bytes(tbl, 0, mem::size_of::<[Entry; 512]>()) };
		self.0 = frame | Self::PRESENT;
		// SAFETY: the table is properly initialized.
		unsafe { &mut *tbl }
	}

	pub fn set_page(&mut self, frame: u64) -> Result<(), SetPageError> {
		if self.is_table() {
			Err(SetPageError::IsTable)
		} else if self.is_leaf() {
			Err(SetPageError::IsMapped)
		} else {
			self.0 = frame | Self::PRESENT;
			Ok(())
		}
	}

	/// # Safety
	///
	/// Identity mappings must still be present.
	pub fn new_table_phys(&mut self, frame: frame::PageFrame) -> &mut [Entry; 512] {
		assert_eq!(frame.p2size, 0);
		let frame: u64 = frame.base.try_into().unwrap();
		// SAFETY: FIXME the allocator makes no guarantees about the address of the frame.
		let tbl = unsafe { frame as *mut [Entry; 512] };
		// SAFETY: a fully zeroed Entry is valid.
		dbg!();
		loop {}
		unsafe { ptr::write_bytes(tbl, 0, mem::size_of::<[Entry; 512]>()) };
		dbg!();
		self.0 = frame | Self::PRESENT;
		// SAFETY: the table is properly initialized.
		unsafe { &mut *tbl }
	}
}

#[derive(Debug)]
pub enum MakeTableError {
	IsMapped,
	OutOfFrames,
}

#[derive(Debug)]
pub enum SetPageError {
	IsMapped,
	IsTable,
}

impl From<frame::AllocateError> for MakeTableError {
	fn from(err: frame::AllocateError) -> Self {
		match err {
			frame::AllocateError::OutOfFrames => Self::OutOfFrames,
		}
	}
}

pub fn get_entry_mut(
	table: &mut [Entry; 512],
	address: u64,
	level: u8,
	depth: u8,
) -> Result<&mut Entry, (&mut Entry, u8)> {
	let offt = usize::try_from((address >> (12 + u64::from(level + depth) * 9)) & 0x1ff).unwrap();
	let entry = &mut table[offt];
	if depth == 0 {
		Ok(entry)
	} else if entry.is_table() {
		// The borrow checked is retarded, so this will have to do.
		let table = entry.as_table_mut().unwrap();
		get_entry_mut(table, address, level, depth - 1)
	} else {
		Err((entry, depth))
	}
}

pub fn get_current<'a>() -> &'a mut [Entry; 512] {
	unsafe {
		let phys: u64;
		asm!("mov %cr3, {0}", out(reg) phys, options(att_syntax));
		&mut *phys_to_virt(phys & !0xfff).cast()
	}
}

/// # Safety
///
/// `virt` must point to a location inside the idempotent map.
unsafe fn virt_to_phys(virt: *const u8) -> u64 {
	debug_assert!(
		IDEMPOTENT_MAP_ADDRESS as *const _ <= virt && virt <= u64::MAX as *const _,
		"virt out of range"
	);
	virt.offset_from(IDEMPOTENT_MAP_ADDRESS).try_into().unwrap()
}

/// # Safety
///
/// `phys` must be in range, i.e. lower than `1 << 46`.
unsafe fn phys_to_virt(phys: u64) -> *mut u8 {
	debug_assert!(phys < 1 << 46, "phys out of range");
	IDEMPOTENT_MAP_ADDRESS.add(usize::try_from(phys).unwrap())
}