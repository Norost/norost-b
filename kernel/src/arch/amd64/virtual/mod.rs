mod address_space;
mod common;
mod pml4;

pub use address_space::{AddressSpace, MapError};
pub use common::{phys_to_virt, virt_to_phys};
pub use pml4::*;

use crate::memory::frame::{PageFrameIter, PPN};
use crate::memory::r#virtual::RWX;
use crate::memory::Page;
use core::ptr::NonNull;

#[derive(Debug)]
pub enum IdentityMapError {
	OutOfFrames,
}

pub unsafe fn add_identity_mapping(phys: usize, size: usize) -> Result<bool, IdentityMapError> {
	assert_eq!(phys & 0xfff, 0, "base address is not aligned");
	assert_eq!(size & 0xfff, 0, "size is not a multiple of the page size");
	unsafe {
		let virt = phys_to_virt(phys.try_into().unwrap()).cast();
		let iter = PageFrameIter {
			base: PPN::from_ptr(virt),
			count: size / 4096,
		};

		match AddressSpace::kernel_map(virt, iter, RWX::RW) {
			Ok(_) => Ok(true),
			Err(MapError::AlreadyMapped) => Ok(false),
			Err(MapError::OutOfFrames) => Err(IdentityMapError::OutOfFrames),
		}
	}
}

/// # Safety
///
/// This function may only be called once.
pub(super) unsafe fn init() {
	unsafe { address_space::init() }
}
