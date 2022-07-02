#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;

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

	#[inline]
	pub fn area(&self) -> usize {
		usize::from(self.x + 1) * usize::from(self.y + 1)
	}
}

pub struct DrawRect<'a, T>
where
	T: AsRef<[u8]> + 'a,
{
	raw: T,
	_marker: PhantomData<&'a mut T>,
}

impl<'a, T> DrawRect<'a, T>
where
	T: AsRef<[u8]> + 'a,
{
	pub fn from_bytes(raw: T) -> Option<Self> {
		(12 + Self::get_size(raw.as_ref())?.area() * 3 >= raw.as_ref().len()).then(|| Self {
			raw,
			_marker: PhantomData,
		})
	}

	pub fn origin(&self) -> Point {
		Point::from_raw(self.raw.as_ref()[..8].try_into().unwrap())
	}

	pub fn size(&self) -> Size {
		Self::get_size(self.raw.as_ref()).unwrap()
	}

	pub fn pixels(&self) -> &[u8] {
		&self.raw.as_ref()[12..12 + self.size().area() * 3]
	}

	fn get_size(raw: &[u8]) -> Option<Size> {
		Some(Size::from_raw(raw.get(8..12)?.try_into().unwrap()))
	}
}

impl<'a, T> DrawRect<'a, T>
where
	T: AsRef<[u8]> + AsMut<[u8]> + 'a,
{
	pub fn pixels_mut(&mut self) -> &mut [u8] {
		let s = self.size().area();
		&mut self.raw.as_mut()[12..12 + s * 3]
	}
}

impl<'a> DrawRect<'a, &'a mut Vec<u8>> {
	pub fn new_vec(raw: &'a mut Vec<u8>, origin: Point, size: Size) -> Self {
		raw.clear();
		raw.resize(12 + size.area() * 3, 0);
		raw[8..12].copy_from_slice(&size.to_raw());
		raw[..8].copy_from_slice(&origin.to_raw());
		Self {
			raw: &mut *raw,
			_marker: PhantomData,
		}
	}
}
