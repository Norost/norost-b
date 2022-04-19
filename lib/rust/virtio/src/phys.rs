//! Utilities to deal with physical addresses.

use core::fmt;
use core::ops::{Add, Sub};
use endian::u64le;

/// Representation of a physical address.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PhysAddr(pub u64le);

impl PhysAddr {
	pub fn new(n: u64) -> Self {
		Self(n.into())
	}
}

impl Add<u64> for PhysAddr {
	type Output = Self;

	fn add(self, rhs: u64) -> Self::Output {
		Self((u64::from(self.0) + rhs).into())
	}
}

impl Sub<u64> for PhysAddr {
	type Output = Self;

	fn sub(self, rhs: u64) -> Self::Output {
		Self((u64::from(self.0) + rhs).into())
	}
}

impl fmt::Debug for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#x}", self.0)
	}
}

#[derive(Clone, Copy)]
pub struct PhysRegion {
	pub base: PhysAddr,
	pub size: u32,
}
