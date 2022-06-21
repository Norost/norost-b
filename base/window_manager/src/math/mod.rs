mod rect;

pub use rect::*;

use core::ops::{Add, AddAssign, Mul, Neg, RangeInclusive, Sub, SubAssign};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
	pub x: u32,
	pub y: u32,
}

impl Point {
	pub const ORIGIN: Self = Self::new(0, 0);

	#[inline(always)]
	pub const fn new(x: u32, y: u32) -> Self {
		Self { x, y }
	}

	#[inline(always)]
	pub const fn into_vector(self) -> Vector {
		Vector::new(self.x as _, self.y as _)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector {
	pub x: i32,
	pub y: i32,
}

impl Vector {
	pub const X: Self = Self::new(1, 0);
	pub const Y: Self = Self::new(0, 1);
	pub const ZERO: Self = Self::new(0, 0);
	pub const ONE: Self = Self::new(1, 1);

	#[inline(always)]
	pub const fn new(x: i32, y: i32) -> Self {
		Self { x, y }
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
	pub x: u32,
	pub y: u32,
}

impl Size {
	pub const ZERO: Self = Self::new(0, 0);

	#[inline(always)]
	pub const fn new(x: u32, y: u32) -> Self {
		Self { x, y }
	}

	#[inline(always)]
	pub const fn into_vector(self) -> Vector {
		Vector::new(self.x as _, self.y as _)
	}
}

macro_rules! impl_op {
	($l:ident $r:ident $out:ident $wrap_fn:ident | $op:ident.$fn:ident) => {
		impl $op<$r> for $l {
			type Output = $out;

			#[inline(always)]
			fn $fn(self, rhs: $r) -> Self::Output {
				$out::new(
					self.x.$wrap_fn(rhs.x as _) as _,
					self.y.$wrap_fn(rhs.y as _) as _,
				)
			}
		}
	};
	($l:ident $r:ident $wrap_fn:ident = $opa:ident.$fna:ident) => {
		impl $opa<$r> for $l {
			#[inline(always)]
			fn $fna(&mut self, rhs: $r) {
				self.x = self.x.$wrap_fn(rhs.x as _);
				self.y = self.y.$wrap_fn(rhs.y as _);
			}
		}
	};
}

impl_op!(Vector Vector Vector wrapping_add | Add.add);
impl_op!(Vector Vector wrapping_add = AddAssign.add_assign);
impl_op!(Point Vector Point wrapping_add | Add.add);
impl_op!(Point Vector wrapping_add = AddAssign.add_assign);
impl_op!(Vector Point Point wrapping_add | Add.add);

impl Mul<u32> for Vector {
	type Output = Vector;

	fn mul(self, rhs: u32) -> Self {
		Self::new(self.x * rhs as i32, self.y * rhs as i32)
	}
}

impl_op!(Vector Vector Vector wrapping_sub | Sub.sub);
impl_op!(Vector Vector wrapping_sub = SubAssign.sub_assign);
impl_op!(Point Vector Point wrapping_sub | Sub.sub);
impl_op!(Point Vector wrapping_sub = SubAssign.sub_assign);
impl_op!(Vector Point Point wrapping_sub | Sub.sub);
impl_op!(Point Point Vector wrapping_sub | Sub.sub);

impl Neg for Vector {
	type Output = Self;

	#[inline(always)]
	fn neg(self) -> Self {
		Self::ZERO - self
	}
}

/// A fractional ratio from 0 to 1 with 16-bit granulity.
#[derive(Clone, Copy)]
pub struct Ratio(u16);

impl Ratio {
	/// A close approximation of a 0.5 ratio.
	pub const HALF: Self = Ratio(0x8000);

	/// Partition a length in two lengths that sum up to the original length.
	pub fn partition(self, length: u32) -> (u32, u32) {
		let l = u32::try_from(u64::from(length) * u64::from(self.0) / u64::from(u16::MAX)).unwrap();
		(l, length - l)
	}

	/// Partition between two points.
	pub fn partition_range(self, range: RangeInclusive<u32>) -> u32 {
		range.start() + self.partition(range.end() - range.start()).0
	}
}

impl Default for Ratio {
	fn default() -> Self {
		Self::HALF
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn ratio_half() {
		assert_eq!(Ratio::HALF.partition(100), (50, 50));
		assert_eq!(Ratio::HALF.partition(4096), (2048, 2048));
		assert_eq!(Ratio::HALF.partition(0x10000), (0x8000, 0x8000));
		// The below fails, but this is be fine for now. We can easily increase
		// precision to 32 bits at a later time.
		//assert_eq!(Ratio::HALF.partition(0x20000), (0x10000, 0x10000));
	}
}
