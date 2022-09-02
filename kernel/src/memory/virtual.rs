mod address_space;
mod permission_mask;

use super::frame::PPN;

pub use {
	crate::arch::amd64::r#virtual::{phys_to_virt, virt_to_phys},
	address_space::{MapError, *},
	norostb_kernel::syscall::{IncompatibleRWXFlags, RWX},
	permission_mask::mask_permissions_object,
};

pub unsafe trait Mappable<I>
where
	I: ExactSizeIterator<Item = PPN>,
{
	fn len(&self) -> usize;

	fn frames(&self) -> I;
}
