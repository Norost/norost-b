mod address_space;

use super::frame::PPN;
pub use crate::arch::amd64::r#virtual::{add_identity_mapping, phys_to_virt, virt_to_phys};
pub use address_space::{MapError, *};

pub unsafe trait Mappable<I>
where
	I: ExactSizeIterator<Item = PPN>,
{
	fn len(&self) -> usize;

	fn frames(&self) -> I;
}

#[derive(Clone, Copy, Debug)]
pub enum RWX {
	R,
	W,
	X,
	RW,
	RX,
	RWX,
}

impl RWX {
	pub fn from_flags(r: bool, w: bool, x: bool) -> Result<RWX, IncompatibleRWXFlags> {
		match (r, w, x) {
			(true, false, false) => Ok(Self::R),
			(false, true, false) => Ok(Self::W),
			(false, false, true) => Ok(Self::X),
			(true, true, false) => Ok(Self::RW),
			(true, false, true) => Ok(Self::RX),
			(true, true, true) => Ok(Self::RWX),
			_ => Err(IncompatibleRWXFlags),
		}
	}

	pub fn w(&self) -> bool {
		match self {
			Self::W | Self::RW | Self::RWX => true,
			Self::R | Self::X | Self::RX => false,
		}
	}
}

#[derive(Debug)]
pub struct IncompatibleRWXFlags;
