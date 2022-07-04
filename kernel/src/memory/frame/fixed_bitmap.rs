//! Bitmap-based allocator. Intended for DMA allocations.

use super::{NonZeroUsize, Page, PPN};
use crate::boot::MemoryRegion;
use core::ops::{BitAnd, BitAndAssign, Not, Shl, Shr};

pub(super) struct FixedBitmap {
	base: PPN,
	bitmap: Bitmap<32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Bitmap<const N: usize>([u128; N]); // 512K * 32 = 16M, plenty big FB for my monitor :)

impl FixedBitmap {
	pub fn add_region(&mut self, mr: &mut MemoryRegion) {
		if self.bitmap.0[0] & 1 != 0 {
			return;
		}
		if let Some(base) = mr.take_page_range(128 * self.bitmap.0.len()) {
			self.base = PPN((base >> Page::OFFSET_BITS)
				.try_into()
				.expect("TODO: page address out of range"));
			self.bitmap.0.iter_mut().for_each(|n| *n = u128::MAX);
		}
	}

	pub fn pop_range(&mut self, n: NonZeroUsize) -> Option<PPN> {
		if n.get() > 128 * self.bitmap.0.len() {
			return None;
		}
		let mut mask = Bitmap::ones(n.get());
		let mut shift = 0usize;
		while self.bitmap & mask != mask {
			shift += 1;
			if shift == 128 * self.bitmap.0.len() {
				return None;
			}
			mask = mask << 1;
		}
		self.bitmap &= !mask;
		Some(self.base.skip(shift as _))
	}
}

impl const Default for FixedBitmap {
	fn default() -> Self {
		Self {
			base: PPN(0),
			bitmap: Bitmap([0; 32]),
		}
	}
}

impl<const N: usize> Bitmap<N> {
	fn ones(mut n: usize) -> Self {
		let mut a = [0; N];
		let mut i = 0;
		while let Some(nn) = n.checked_sub(128) {
			a[i] = u128::MAX;
			n = nn;
			i += 1;
		}
		if n > 0 {
			a[i] = (1u128 << n) - 1;
		}
		Self(a)
	}
}

impl<const N: usize> BitAnd<Self> for Bitmap<N> {
	type Output = Self;

	fn bitand(mut self, rhs: Self) -> Self {
		self &= rhs;
		self
	}
}

impl<const N: usize> BitAndAssign<Self> for Bitmap<N> {
	fn bitand_assign(&mut self, rhs: Self) {
		for (a, b) in self.0.iter_mut().zip(&rhs.0) {
			*a &= *b
		}
	}
}

impl<const N: usize> Shl<usize> for Bitmap<N> {
	type Output = Self;

	fn shl(mut self, rhs: usize) -> Self {
		assert!(rhs < 128, "TODO");
		for i in (0..self.0.len()).rev() {
			let a = self.0[i];
			let b = i.checked_sub(1).map_or(0, |i| self.0[i]);
			self.0[i] = a << rhs | b >> (128 - rhs);
		}
		self
	}
}

impl<const N: usize> Shr<usize> for Bitmap<N> {
	type Output = Self;

	fn shr(mut self, rhs: usize) -> Self {
		assert!(rhs < 128, "TODO");
		for i in 0..self.0.len() {
			let a = self.0[i];
			let b = *self.0.get(i + 1).unwrap_or(&0);
			self.0[i] = b << (128 - rhs) | a >> rhs;
		}
		self
	}
}

impl<const N: usize> Not for Bitmap<N> {
	type Output = Self;

	fn not(mut self) -> Self {
		self.0.iter_mut().for_each(|n| *n = !*n);
		self
	}
}
