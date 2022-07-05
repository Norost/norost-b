mod address_space;
mod permission_mask;

use super::frame::PPN;

pub use crate::arch::amd64::r#virtual::{add_identity_mapping, phys_to_virt, virt_to_phys};
pub use address_space::{MapError, *};
pub use norostb_kernel::syscall::{IncompatibleRWXFlags, RWX};
pub use permission_mask::{mask_permissions, mask_permissions_object};

pub unsafe trait Mappable<I>
where
	I: ExactSizeIterator<Item = PPN>,
{
	fn len(&self) -> usize;

	fn frames(&self) -> I;
}
