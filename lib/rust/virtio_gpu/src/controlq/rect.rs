use super::*;
use core::fmt;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Rect {
	x: u32le,
	y: u32le,
	width: u32le,
	height: u32le,
}

impl Rect {
	pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
		Self {
			x: x.into(),
			y: y.into(),
			width: width.into(),
			height: height.into(),
		}
	}

	#[inline(always)]
	pub fn x(&self) -> u32 {
		self.x.into()
	}

	#[inline(always)]
	pub fn y(&self) -> u32 {
		self.y.into()
	}

	#[inline(always)]
	pub fn width(&self) -> u32 {
		self.width.into()
	}

	#[inline(always)]
	pub fn height(&self) -> u32 {
		self.height.into()
	}

	#[inline(always)]
	pub fn set_x(&mut self, x: u32) {
		self.x = x.into();
	}

	#[inline(always)]
	pub fn set_y(&mut self, y: u32) {
		self.y = y.into();
	}

	#[inline(always)]
	pub fn set_width(&mut self, width: u32) {
		self.width = width.into();
	}

	#[inline(always)]
	pub fn set_height(&mut self, height: u32) {
		self.height = height.into();
	}
}

impl fmt::Debug for Rect {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Rect))
			.field("x", &self.x())
			.field("y", &self.y())
			.field("width", &self.width())
			.field("height", &self.height())
			.finish()
	}
}
