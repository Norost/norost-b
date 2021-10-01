use super::common;
use crate::memory::Page;
use crate::memory::frame;
use crate::memory::r#virtual::phys_to_virt;

pub struct AddressSpace {
	cr3: usize,
}

impl AddressSpace {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let ppn = frame::allocate_contiguous(1)?;
		let mut slf = Self { cr3: ppn.as_phys() };
		
		// Map the kernel pages
		let cur = unsafe { Self::current() };
		for (w, r) in unsafe { slf.table_mut() }[256..].iter_mut().zip(cur[256..].iter()) {
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
	pub unsafe fn map(&mut self, address: *const Page, frames: impl ExactSizeIterator<Item = frame::PPN>, hint_color: u8) -> Result<(), MapError> {
		let tbl = self.table_mut();
		dbg!(tbl as *mut _);
		for (i, f) in frames.enumerate() {
			loop {
				match common::get_entry_mut(tbl, address.add(i) as u64, 0, 3) {
					Ok(e) => {
						dbg!(e as *mut _);
						e.set_page(f.as_phys() as u64, true).unwrap();
						dbg!(e);
						dbg!(address.add(i), f.as_phys());
						break;
					}
					Err((e, d)) => {
						dbg!(e as *mut _);
						e.make_table(true, hint_color).unwrap();
						dbg!(d);
					}
				}
			}
		}
		Ok(())
	}

	pub unsafe fn activate(&self) {
		asm!("mov cr3, {0}", in(reg) self.cr3);
	}

	unsafe fn table_mut(&mut self) -> &mut [common::Entry; 512] {
		&mut *phys_to_virt((self.cr3 & !Page::OFFSET_MASK) as u64).cast()
	}

	unsafe fn current<'a>() -> &'a mut [common::Entry; 512] {
		let cr3: usize;
		asm!("mov {0}, cr3", out(reg) cr3);
		&mut *phys_to_virt((cr3 & !Page::OFFSET_MASK) as u64).cast()
	}
}

impl Drop for AddressSpace {
	fn drop(&mut self) {
		todo!()
	}
}

#[derive(Debug)]
pub enum MapError {

}
