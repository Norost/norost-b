use crate::msr::rdmsr;

pub struct Cap(u64);

impl Cap {
	/// # Safety
	///
	/// MTRRs must be supported.
	pub unsafe fn get() -> Self {
		Self(rdmsr(0xfe))
	}

	pub fn range_count(&self) -> u8 {
		(self.0 & 0xff) as u8
	}
}

pub struct Range {
	base: u64,
	mask: u64,
}

impl Range {
	/// # Safety
	///
	/// MTRR has to exist, i.e. `id < Cap::new().range_count()`.
	#[must_use = "RDMSR cannot be optimized out"]
	pub unsafe fn get(id: u8) -> Option<Self> {
		let id = u32::from(id) * 2;
		let mask = rdmsr(0x201 + id);
		((mask & 1 << 11) > 0).then(|| {
			Self {
				base: rdmsr(0x200 + id),
				mask: mask & !0xfff,
			}
		})
	}

	/// Checks whether an address is inside this range.
	pub fn contains(&self, address: u64) -> bool {
		// Stolen from https://wiki.osdev.org/MTRR#IA32_MTRRphysBasen_and_IA32_MTRRphysMaskn_registers
		(self.mask & self.base & 0xf_ffff_ffff_f000) == (self.mask & address)
	}

	/// Check whether this range intersects with the given 2MB frame.
	pub fn intersects_2mb(&self, address: u64) -> bool {
		debug_assert_eq!(address & 0x1f_ffff, 0, "2MB frame not aligned");
		let (mut yes, mut no) = (false, false);
		for i in 0..512 {
			yes |= self.contains(address + i << 12);
			no |= !self.contains(address + i << 12);
			if yes && no {
				return true;
			}
		}
		false
	}

	/// Check whether this range intersects with the given 1GB frame. Returns the index of the first
	/// 2MB this range intersects.
	pub fn intersects_1gb(&self, address: u64) -> Option<usize> {
		debug_assert_eq!(address & 0x3fff_ffff, 0, "1GB frame not aligned");
		for i in 0..512u16 {
			if self.intersects_2mb(address + u64::from(i) << 21) {
				return Some(usize::from(i));
			}
		}
		None
	}
}

// TODO this implementation scales terribly. It would be much better to prefetch the ranges (
// and assume the MTRRs are sane too).
pub struct AllRanges {
	count: u8,
}

impl AllRanges {
	/// # Safety
	///
	/// MTRRs must be supported.
	pub unsafe fn new() -> Self {
		Self {
			count: Cap::get().range_count(),
		}
	}

	/// Check whether the given 2MB frame intersects with any range.
	pub fn intersects_2mb(&self, address: u64) -> bool {
		// SAFETY: All MTRRs up to count exist.
		(0..self.count).filter_map(|i| unsafe { Range::get(i) }).any(|r| r.intersects_2mb(address))
	}

	/// Check whether the given 1GB frame intersects with any range. Returns the index of the first
	/// 2MB this range intersects.
	pub fn intersects_1gb(&self, address: u64) -> Option<usize> {
		// SAFETY: All MTRRs up to count exist.
		(0..self.count)
			.filter_map(|i| unsafe { Range::get(i) })
			.filter_map(|r| r.intersects_1gb(address))
			.min()
	}
}
