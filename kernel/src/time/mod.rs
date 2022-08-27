use core::{fmt, time::Duration};

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Monotonic {
	ns: u64,
}

impl Monotonic {
	pub const ZERO: Self = Self { ns: 0 };
	pub const MAX: Self = Self { ns: u64::MAX };

	pub fn from_nanos(ns: u64) -> Self {
		Self { ns }
	}

	pub fn as_nanos(&self) -> u64 {
		self.ns
	}

	#[cfg(not(feature = "driver-hpet"))]
	pub fn from_seconds(s: u128) -> Self {
		Self {
			nanoseconds: (s * 1_000_000_000)
				.try_into()
				.expect("seconds too far in the future"),
		}
	}

	pub fn checked_add(self, dt: Duration) -> Option<Self> {
		u64::try_from(dt.as_nanos())
			.ok()
			.and_then(|dt| self.ns.checked_add(dt))
			.map(|ns| Self { ns })
	}

	pub fn checked_add_nanos(self, dt: u64) -> Option<Self> {
		self.ns.checked_add(dt).map(|ns| Self { ns })
	}

	pub fn saturating_add(self, dt: Duration) -> Self {
		self.checked_add(dt).unwrap_or(Self::MAX)
	}

	pub fn saturating_add_nanos(self, dt: u64) -> Self {
		self.checked_add_nanos(dt).unwrap_or(Self::MAX)
	}

	/// Returns the `Duration` until the given `Monotonic`. This is `None` if the
	/// given `Monotonic` has already passed.
	pub fn duration_until(self, until: Self) -> Option<Duration> {
		until.ns.checked_sub(self.ns).map(Duration::from_nanos)
	}
}

impl fmt::Debug for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		core::time::Duration::from_nanos(self.ns).fmt(f)
	}
}
