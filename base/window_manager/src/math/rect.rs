use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
	low: Point,
	high: Point,
}

impl Rect {
	/// `a` and `b` are *inclusive*.
	#[inline]
	pub fn from_points(a: Point, b: Point) -> Self {
		Self {
			low: Point::new(a.x.min(b.x), a.y.min(b.y)),
			high: Point::new(a.x.max(b.x), a.y.max(b.y)),
		}
	}

	pub fn from_size(low: Point, size: Size) -> Self {
		Self { low, high: low + size.into_vector() - Vector::ONE }
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

	#[inline]
	pub fn area(&self) -> u64 {
		self.size().area()
	}

	#[inline(always)]
	pub const fn x(&self) -> RangeInclusive<u32> {
		self.low.x..=self.high.x
	}

	#[inline(always)]
	pub const fn y(&self) -> RangeInclusive<u32> {
		self.low.y..=self.high.y
	}

	pub fn contains(&self, point: Point) -> bool {
		self.x().contains(&point.x) && self.y().contains(&point.y)
	}

	/// Try to place a [`Rect`] in this [`Rect`]'s local space in this parent's local space.
	pub fn calc_global_pos(&self, rect: Rect) -> Option<Rect> {
		self.contains(self.low() + rect.high().into_vector())
			.then(|| Self::from_size(self.low() + rect.low().into_vector(), rect.size()))
	}
}
