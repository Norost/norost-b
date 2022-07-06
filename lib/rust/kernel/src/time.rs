use core::{fmt, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Monotonic {
	ns: u64,
}

impl Monotonic {
	#[inline]
	pub fn now() -> Self {
		crate::syscall::monotonic_time()
	}

	#[inline]
	pub fn from_nanos(ns: u64) -> Self {
		Self { ns }
	}

	#[inline]
	pub fn as_nanos(&self) -> u64 {
		self.ns
	}

	#[inline]
	pub fn as_micros(&self) -> u64 {
		self.ns / 1_000
	}

	#[inline]
	pub fn as_millis(&self) -> u64 {
		self.ns / 1_000_000
	}

	#[inline]
	pub fn as_secs(&self) -> u64 {
		self.ns / 1_000_000_000
	}

	#[inline]
	pub fn checked_duration_since(&self, earlier: Monotonic) -> Option<Duration> {
		self.ns
			.checked_sub(earlier.ns)
			.map(Into::into)
			.map(Duration::from_nanos)
	}

	#[inline]
	pub fn duration_since(&self, earlier: Monotonic) -> Duration {
		self.checked_duration_since(earlier).unwrap_or_default()
	}
}

impl fmt::Debug for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		Duration::from_nanos(self.ns).fmt(f)
	}
}

impl fmt::Display for Monotonic {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Debug::fmt(self, f)
	}
}
