mod address_space;
mod common;
mod pml4;

pub use {
	address_space::{AddressSpace, MapError},
	common::{phys_to_virt, virt_to_phys},
	pml4::*,
};

/// # Safety
///
/// This function may only be called once.
pub(super) unsafe fn init() {
	unsafe { address_space::init() }
}
