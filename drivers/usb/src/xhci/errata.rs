/// Observed errata and workarounds.
pub struct Errata(u64);

macro_rules! errata {
	($err:ident $fn:ident $n:literal) => {
		const $err: u64 = 1 << $n;

		pub fn $fn(&self) -> bool {
			self.0 & Self::$err != 0
		}
	};
}

impl Errata {
	errata!(NO_PSCE_ON_RESET no_psce_on_reset 0);
	errata!(HANG_AFTER_RESET hang_after_reset 1);

	pub const NONE: Self = Self(0);
	pub const PCI_1B36_000D: Self = Self(Self::NO_PSCE_ON_RESET);

	pub fn get(vendor: u16, device: u16) -> Self {
		match (vendor, device) {
			(0x1b36, 0x000d) => Self::PCI_1B36_000D,
			_ => Self::NONE,
		}
	}

	pub fn set_intel_vendor(&mut self) {
		self.0 |= Self::HANG_AFTER_RESET;
	}
}
