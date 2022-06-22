#![no_std]

extern crate alloc;

use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct Point {
	pub x: u32,
	pub y: u32,
}

impl Point {
	fn from_raw(r: [u8; 8]) -> Self {
		Self {
			x: u32::from_le_bytes(r[..4].try_into().unwrap()),
			y: u32::from_le_bytes(r[4..].try_into().unwrap()),
		}
	}

	fn to_raw(&self) -> [u8; 8] {
		let mut r = [0; 8];
		r[..4].copy_from_slice(&self.x.to_le_bytes());
		r[4..].copy_from_slice(&self.y.to_le_bytes());
		r
	}
}

/// Each component is encoded as the size minus 1, e.g. `16` is encoded as `15`,
/// `65536` is encoded as `65535` (`0xffff`).
#[derive(Clone, Copy, Debug)]
pub struct Size {
	pub x: u16,
	pub y: u16,
}

impl Size {
	fn from_raw(r: [u8; 4]) -> Self {
		Self {
			x: u16::from_le_bytes(r[..2].try_into().unwrap()),
			y: u16::from_le_bytes(r[2..].try_into().unwrap()),
		}
	}

	fn to_raw(&self) -> [u8; 4] {
		let mut r = [0; 4];
		r[..2].copy_from_slice(&self.x.to_le_bytes());
		r[2..].copy_from_slice(&self.y.to_le_bytes());
		r
	}

	fn area(&self) -> usize {
		usize::from(self.x + 1) * usize::from(self.y + 1)
	}
}

pub struct DrawRect {
	pub raw: Vec<u8>,
}

impl DrawRect {
	pub fn new(mut raw: Vec<u8>) -> Self {
		raw.clear();
		raw.resize(12, 0);
		Self { raw }
	}

	pub fn origin(&self) -> Option<Point> {
		Some(Point::from_raw(self.raw.get(..8)?.try_into().unwrap()))
	}

	pub fn size(&self) -> Option<Size> {
		Some(Size::from_raw(self.raw.get(8..12)?.try_into().unwrap()))
	}

	pub fn set_origin(&mut self, origin: Point) {
		self.raw[..8].copy_from_slice(&origin.to_raw())
	}

	pub fn set_size(&mut self, size: Size) {
		self.raw.resize(12 + size.area() * 3, 0);
		self.raw[8..12].copy_from_slice(&size.to_raw());
	}

	pub fn pixels(&self) -> Option<&[u8]> {
		self.raw.get(12..12 + self.size()?.area() * 3)
	}

	pub fn pixels_mut(&mut self) -> Option<&mut [u8]> {
		let s = self.size()?.area();
		self.raw.get_mut(12..12 + s * 3)
	}
}
