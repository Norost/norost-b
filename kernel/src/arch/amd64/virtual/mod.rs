mod address_space;
mod common;
mod pml4;

pub use address_space::{AddressSpace, MapError};
pub use common::{phys_to_virt, virt_to_phys};
pub use pml4::*;

use crate::memory::frame::{PageFrame, PPN};
use crate::memory::r#virtual::RWX;
use crate::memory::Page;
use core::ptr::NonNull;

pub unsafe fn add_identity_mapping(phys: usize, size: usize) -> Result<NonNull<Page>, ()> {
	struct Iter {
		phys: PPN,
		size: usize,
	}

	impl Iterator for Iter {
		type Item = PageFrame;

		fn next(&mut self) -> Option<Self::Item> {
			if self.size >= 1 << 9 && self.phys.as_phys() & 0x1ff_fff == 0 {
				let f = PageFrame {
					base: self.phys,
					p2size: 9,
				};
				self.size -= 1 << 9;
				self.phys = self.phys.skip(1 << 9);
				Some(f)
			} else if self.size > 0 {
				let f = PageFrame {
					base: self.phys,
					p2size: 1,
				};
				self.size -= 1;
				self.phys = self.phys.skip(1);
				Some(f)
			} else {
				None
			}
		}
	}

	impl ExactSizeIterator for Iter {
		fn len(&self) -> usize {
			todo!()
		}
	}

	unsafe {
		let virt = phys_to_virt(phys.try_into().unwrap()).cast();
		let iter = Iter {
			phys: PPN::from_ptr(virt),
			size: size / 4096,
		};

		AddressSpace::kernel_map(virt, iter, RWX::RW).unwrap();

		Ok(NonNull::new_unchecked(virt))
	}
}
