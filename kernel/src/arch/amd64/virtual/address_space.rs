use super::common;
use crate::memory::frame::{self, PPN};
use crate::memory::r#virtual::{phys_to_virt, RWX};
use crate::memory::Page;
use core::arch::asm;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

pub struct AddressSpace {
	cr3: usize,
}

/// The default address space. This should be used when no process is active.
static mut DEFAULT_ADDRESS_SPACE: MaybeUninit<AddressSpace> = MaybeUninit::uninit();

impl AddressSpace {
	pub fn new() -> Result<Self, frame::AllocateError> {
		let mut ppn = None;
		frame::allocate(1, |f| ppn = Some(f), 0 as _, 0)?;
		let ppn = ppn.unwrap();
		unsafe {
			ppn.as_ptr().cast::<Page>().write_bytes(0, 1);
		}
		let mut slf = Self { cr3: ppn.as_phys() };

		// Map the kernel pages
		let cur = unsafe { Self::current() };
		for (w, r) in unsafe { slf.table_mut() }[256..]
			.iter_mut()
			.zip(cur[256..].iter())
		{
			*w = r.clone();
		}

		Ok(slf)
	}

	/// Map the frames to the given address. This will override any existing mappings.
	///
	/// This is intended for mapping into userspace only.
	///
	/// # Safety
	///
	/// If the mappings already existed, they must be flushed from the TLB.
	pub unsafe fn map(
		&mut self,
		address: *const Page,
		frames: impl ExactSizeIterator<Item = frame::PPN>,
		rwx: RWX,
		hint_color: u8,
	) -> Result<(), MapError> {
		unsafe { Self::map_common(self.table_mut(), address, frames, rwx, hint_color, true) }
	}

	pub unsafe fn kernel_map(
		address: *const Page,
		frames: impl ExactSizeIterator<Item = frame::PPN>,
		rwx: RWX,
	) -> Result<(), MapError> {
		unsafe { Self::map_common(Self::current(), address, frames, rwx, 0, false) }
	}

	unsafe fn map_common(
		tbl: &mut [common::Entry; 512],
		address: *const Page,
		frames: impl ExactSizeIterator<Item = frame::PPN>,
		rwx: RWX,
		hint_color: u8,
		user: bool,
	) -> Result<(), MapError> {
		for (i, ppn) in frames.enumerate() {
			loop {
				match common::get_entry_mut(tbl, address.wrapping_add(i) as u64, 0, 3) {
					Ok(e) => {
						e.set_page(ppn.as_phys() as u64, user, rwx.w()).unwrap();
						break;
					}
					Err((e, _)) => {
						e.make_table(user, hint_color).map_err(|e| match e {
							common::MakeTableError::IsLeaf => MapError::AlreadyMapped,
							common::MakeTableError::OutOfFrames => MapError::OutOfFrames,
						})?;
					}
				}
			}
		}
		Ok(())
	}

	pub unsafe fn unmap(
		&mut self,
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		unsafe { Self::unmap_common(self.table_mut(), address, count) }
	}

	pub unsafe fn kernel_unmap(
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		unsafe { Self::unmap_common(Self::current(), address, count) }
	}

	unsafe fn unmap_common(
		tbl: &mut [common::Entry; 512],
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		for i in 0..count.get() {
			let e = common::get_entry_mut(tbl, address.as_ptr().wrapping_add(i) as u64, 0, 3)
				.map_err(|_| UnmapError::Unset)?;
			e.clear().ok_or(UnmapError::Unset)?;
		}
		// Flush the unmapped addresses from the TLB.
		// TODO avoid flushing the entire TLB.
		unsafe {
			asm!("mov {0}, cr3", "mov cr3, {0}", out(reg) _);
		}
		Ok(())
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		let offt = address.as_ptr() as usize & 0xfff;
		let tbl = unsafe { self.table() };
		// TODO hugepages
		let e = common::get_entry(tbl, address.as_ptr() as u64, 0, 3)?;
		let p = e.page()?;
		// TODO check for executable
		let rwx = match e.is_writeable() {
			true => RWX::RW,
			false => RWX::R,
		};
		Some((p as usize | offt, rwx))
	}

	pub unsafe fn activate(&self) {
		unsafe {
			asm!("mov cr3, {0}", in(reg) self.cr3);
		}
	}

	unsafe fn table(&self) -> &[common::Entry; 512] {
		unsafe { &*phys_to_virt((self.cr3 & !Page::MASK) as u64).cast() }
	}

	unsafe fn table_mut(&mut self) -> &mut [common::Entry; 512] {
		unsafe { &mut *phys_to_virt((self.cr3 & !Page::MASK) as u64).cast() }
	}

	unsafe fn current<'a>() -> &'a mut [common::Entry; 512] {
		unsafe { &mut *phys_to_virt((current_cr3() & !Page::MASK) as u64).cast() }
	}

	/// Activate the default address space.
	///
	/// # Safety
	///
	/// There should be no active pointers to any user-space data
	// TODO should we even be using any pointers to user-space data directly?
	pub unsafe fn activate_default() {
		unsafe {
			DEFAULT_ADDRESS_SPACE.assume_init_ref().activate();
		}
	}
}

impl Drop for AddressSpace {
	fn drop(&mut self) {
		// FIXME this is technically unsafe since cr3 may still be loaded with this
		// address space.
		unsafe {
			// Recursively look for tables & deallocate them
			dealloc(&self.table()[..256]);

			unsafe fn dealloc(entries: &[common::Entry]) {
				for e in entries.iter().filter_map(|e| e.as_table()) {
					unsafe {
						dealloc(e);
						frame::deallocate(1, || PPN::from_ptr(e as *const _ as _)).unwrap()
					}
				}
			}
		}
	}
}

#[derive(Debug)]
pub enum MapError {
	AlreadyMapped,
	OutOfFrames,
}

#[derive(Debug)]
pub enum UnmapError {
	Unset,
}

/// # Safety
///
/// This function may only be called once.
pub(super) unsafe fn init() {
	unsafe {
		DEFAULT_ADDRESS_SPACE.write(AddressSpace { cr3: current_cr3() });
	}
}

unsafe fn current_cr3() -> usize {
	unsafe {
		let cr3: usize;
		asm!("mov {0}, cr3", out(reg) cr3);
		cr3
	}
}
