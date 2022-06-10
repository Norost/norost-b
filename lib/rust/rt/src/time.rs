use core::{fmt, time};
use norostb_kernel::syscall;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Monotonic {
	ns: u64,
}

impl Monotonic {
	#[inline]
	pub fn now() -> Self {
		Self::from_nanos(syscall::monotonic_time())
	}

	#[inline]
	pub fn from_nanos(ns: u64) -> Self {
		Self { ns }
	}

	#[inline]
	pub fn as_nanos(&self) -> u64 {
		self.ns
	}
}

impl fmt::Debug for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		time::Duration::from_nanos(self.ns).fmt(f)
	}
}

impl fmt::Display for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(self, f)
	}
}
