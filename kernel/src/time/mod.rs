#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Monotonic {
	nanoseconds: u64
}

impl Monotonic {
	pub const ZERO: Self = Self { nanoseconds: 0 };

	pub fn from_nanoseconds(ns: u128) -> Self {
		Self { nanoseconds: ns.try_into().expect("nanoseconds too far in the future") }
	}
}
