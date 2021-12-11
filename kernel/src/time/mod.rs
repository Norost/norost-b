use core::time::Duration;
use core::fmt;

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Monotonic {
	nanoseconds: u64
}

impl Monotonic {
	pub const ZERO: Self = Self { nanoseconds: 0 };
	pub const MAX: Self = Self { nanoseconds: u64::MAX };

	pub fn from_nanoseconds(ns: u128) -> Self {
		Self { nanoseconds: ns.try_into().expect("nanoseconds too far in the future") }
	}

	#[allow(dead_code)]
	pub fn from_seconds(s: u128) -> Self {
		Self { nanoseconds: (s * 1_000_000_000).try_into().expect("seconds too far in the future") }
	}

	pub fn checked_add(self, dt: Duration) -> Option<Self> {
		u64::try_from(dt.as_nanos())
			.ok()
			.and_then(|dt| self.nanoseconds.checked_add(dt))
			.map(|nanoseconds| Self { nanoseconds })
	}

	pub fn saturating_add(self, dt: Duration) -> Self {
		self.checked_add(dt).unwrap_or(Self::MAX)
	}	

	/// Returns the `Duration` until the given `Monotonic`. This is `None` if the
	/// given `Monotonic` has already passed.
	pub fn duration_until(self, until: Self) -> Option<Duration> {
		until.nanoseconds.checked_sub(self.nanoseconds).map(Duration::from_nanos)
	}
}

impl fmt::Debug for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_struct(stringify!(Monotonic));
		f.field("seconds", &(self.nanoseconds / 1_000_000_000));
		f.field("nano_seconds", &(self.nanoseconds % 1_000_000_000));
		f.finish()
	}
}
