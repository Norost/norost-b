use super::{
	super::{vsyscall, PageFlags},
	common,
};
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
	pub fn new(hint_color: u8) -> Result<Self, frame::AllocateError> {
		let mut ppn = None;
		frame::allocate(1, |f| ppn = Some(f), 0 as _, hint_color)?;
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

		// Map the vsyscall page
		unsafe {
			let vs = vsyscall::mapping();
			let mut f = Self::map_common(
				slf.table_mut(),
				vs.data_virt_addr as _,
				RWX::R,
				hint_color,
				true,
				PageFlags::default(),
				true,
			);
			f(vs.data_phys_addr).unwrap_or_else(|e| todo!("{:?}", e));
		}

		Ok(slf)
	}

	/// Map frames to the given address. This will override any existing mappings.
	///
	/// This is intended for mapping into userspace only.
	///
	/// # Safety
	///
	/// If the mappings already existed, they must be flushed from the TLB.
	pub unsafe fn map(
		&mut self,
		address: *const Page,
		rwx: RWX,
		hint_color: u8,
		flags: PageFlags,
	) -> impl FnMut(PPN) -> Result<(), MapError> + '_ {
		unsafe {
			Self::map_common(
				self.table_mut(),
				address,
				rwx,
				hint_color,
				true,
				flags,
				false,
			)
		}
	}

	pub unsafe fn kernel_map(
		address: *const Page,
		rwx: RWX,
		flags: PageFlags,
	) -> impl FnMut(PPN) -> Result<(), MapError> + 'static {
		unsafe { Self::map_common(Self::current(), address, rwx, 0, false, flags, true) }
	}

	unsafe fn map_common(
		tbl: &mut [common::Entry; 512],
		mut address: *const Page,
		rwx: RWX,
		hint_color: u8,
		user: bool,
		flags: PageFlags,
		global: bool,
	) -> impl FnMut(PPN) -> Result<(), MapError> + '_ {
		move |ppn| loop {
			match common::get_entry_mut(tbl, address as u64, 0, 3) {
				Ok(e) => {
					debug_assert!(!e.is_present(), "page already set");
					e.set_page(ppn.as_phys() as u64, user, rwx.w(), flags, global)
						.unwrap();
					address = address.wrapping_add(1);
					break Ok(());
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

	pub unsafe fn unmap(
		&mut self,
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		debug_assert_eq!(
			address.as_ptr() as usize & 0xffff_8000_0000_0000,
			0,
			"attempt to unmap non-user page as user page"
		);
		unsafe { Self::unmap_common(self.table_mut(), address, count) }
	}

	pub unsafe fn kernel_unmap(
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		debug_assert_eq!(
			address.as_ptr() as usize & 0xffff_8000_0000_0000,
			0xffff_8000_0000_0000,
			"attempt to unmap non-kernel page as kernel page"
		);
		unsafe { Self::unmap_common(Self::current(), address, count) }
	}

	unsafe fn unmap_common(
		tbl: &mut [common::Entry; 512],
		address: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		for i in 0..count.get() {
			let addr = NonNull::new(address.as_ptr().wrapping_add(i)).unwrap();
			let e = common::get_entry_mut(tbl, addr.as_ptr() as u64, 0, 3)
				.map_err(|_| UnmapError::Unset)?;
			e.clear().ok_or(UnmapError::Unset)?;
			// Flush the unmapped addresses from the TLB.
			common::invalidate_page(addr);
		}
		Ok(())
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
			debug_assert_ne!(self.cr3, current_cr3(), "address space is still in use");
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
