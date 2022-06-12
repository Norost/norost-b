mod address_space;
mod common;
mod pml4;

pub use address_space::{AddressSpace, MapError};
pub use common::{phys_to_virt, virt_to_phys};
pub use pml4::*;

use crate::memory::frame::{PageFrameIter, PPN};
use crate::memory::r#virtual::RWX;

#[derive(Debug)]
pub enum IdentityMapError {
	OutOfFrames,
}

pub unsafe fn add_identity_mapping(phys: usize, size: usize) -> Result<bool, IdentityMapError> {
	assert_eq!(phys & 0xfff, 0, "base address is not aligned");
	assert_eq!(size & 0xfff, 0, "size is not a multiple of the page size");
	unsafe {
		let virt = phys_to_virt(phys.try_into().unwrap()).cast();
		let mut mapper = AddressSpace::kernel_map(virt, RWX::RW);
		let iter = PageFrameIter {
			base: PPN::from_ptr(virt),
			count: size / 4096,
		};
		let mut added = false;
		for p in iter {
			match mapper(p) {
				Ok(_) => added = true,
				Err(MapError::AlreadyMapped) => {}
				Err(MapError::OutOfFrames) => todo!("{:?}", IdentityMapError::OutOfFrames),
			}
		}
		Ok(added)
	}
}

/// # Safety
///
/// This function may only be called once.
pub(super) unsafe fn init() {
	unsafe { address_space::init() }
}
