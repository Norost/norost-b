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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
	low: Point,
	high: Point,
}

impl Rect {
	/// `a` and `b` are *inclusive*.
	#[inline]
	pub const fn new(a: Point, b: Point) -> Self {
		const fn min(a: u32, b: u32) -> u32 {
			if a < b {
				a
			} else {
				b
			}
		}
		const fn max(a: u32, b: u32) -> u32 {
			if a > b {
				a
			} else {
				b
			}
		}
		Self {
			low: Point::new(min(a.x, b.x), min(a.y, b.y)),
			high: Point::new(max(a.x, b.x), max(a.y, b.y)),
		}
	}

	pub fn from_size(low: Point, size: Size) -> Self {
		Self {
			low,
			high: low + size.into_vector() - Vector::ONE,
		}
	}

	pub fn from_ranges(x: RangeInclusive<u32>, y: RangeInclusive<u32>) -> Self {
		Self {
			low: Point::new(*x.start(), *y.start()),
			high: Point::new(*x.end(), *y.end()),
		}
	}

	/// Low point is *inclusive*.
	#[inline(always)]
	pub const fn low(&self) -> Point {
		self.low
	}

	/// High point is *inclusive*.
	#[inline(always)]
	pub const fn high(&self) -> Point {
		self.high
	}

	#[inline]
	pub fn size(&self) -> Size {
		let Vector { x, y } = self.high - self.low + Vector::ONE;
		Size::new(x as _, y as _)
	}

	#[inline(always)]
	pub const fn x(&self) -> RangeInclusive<u32> {
		self.low.x..=self.high.x
	}

	#[inline(always)]
	pub const fn y(&self) -> RangeInclusive<u32> {
		self.low.y..=self.high.y
	}
}

/// A fractional ratio from 0 to 1 with 16-bit granulity.
#[derive(Clone, Copy)]
pub struct Ratio(u16);

impl Ratio {
	/// A close approximation of a 0.5 ratio.
	pub const HALF: Self = Ratio(0x7fff);

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
