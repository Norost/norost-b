use super::common;
use crate::memory::frame;
use crate::memory::r#virtual::{phys_to_virt, RWX};
use crate::memory::Page;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

pub struct AddressSpace {
	cr3: usize,
}

impl AddressSpace {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let ppn = frame::allocate_contiguous(NonZeroUsize::new(1).unwrap())?;
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
		let tbl = self.table_mut();
		for (i, f) in frames.enumerate() {
			loop {
				match common::get_entry_mut(tbl, address.add(i) as u64, 0, 3) {
					Ok(e) => {
						e.set_page(f.as_phys() as u64, true, rwx.w()).unwrap();
						break;
					}
					Err((e, _)) => {
						e.make_table(true, hint_color).unwrap();
					}
				}
			}
		}
		Ok(())
	}

	pub unsafe fn kernel_map(
		mut address: *const Page,
		frames: impl ExactSizeIterator<Item = frame::PageFrame>,
		rwx: RWX,
	) -> Result<(), MapError> {
		let tbl = Self::current();
		for f in frames {
			let level = (f.p2size >= 9).then(|| 1).unwrap_or(0);
			let offset = (f.p2size >= 9).then(|| 1 << 9).unwrap_or(1);
			let count = (f.p2size >= 9).then(|| 1 << (f.p2size - 9)).unwrap_or(1);
			let mut ppn = f.base;
			for _ in 0..count {
				loop {
					match common::get_entry_mut(tbl, address as u64, level, 3 - level) {
						Ok(e) => {
							e.set_page(ppn.as_phys() as u64, false, rwx.w()).unwrap();
							address = address.add(offset);
							break;
						}
						Err((e, _)) => {
							e.make_table(false, 0).unwrap();
						}
					}
				}
				ppn = ppn.skip(offset.try_into().unwrap());
			}
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
		asm!("mov cr3, {0}", in(reg) self.cr3);
	}

	unsafe fn table(&self) -> &[common::Entry; 512] {
		&*phys_to_virt((self.cr3 & !Page::MASK) as u64).cast()
	}

	unsafe fn table_mut(&mut self) -> &mut [common::Entry; 512] {
		&mut *phys_to_virt((self.cr3 & !Page::MASK) as u64).cast()
	}

	unsafe fn current<'a>() -> &'a mut [common::Entry; 512] {
		let cr3: usize;
		asm!("mov {0}, cr3", out(reg) cr3);
		&mut *phys_to_virt((cr3 & !Page::MASK) as u64).cast()
	}
}

impl Drop for AddressSpace {
	fn drop(&mut self) {
		//todo!()
	}
}

#[derive(Debug)]
pub enum MapError {}
