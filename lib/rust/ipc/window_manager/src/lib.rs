#![no_std]

mod raw {
	norost_ipc_spec::compile!(core::include_str!("../../../../ipc/window_manager.ipc"));
}

use norost_ipc_spec::Data;

#[derive(Clone, Copy, Debug)]
pub struct Point {
	pub x: u32,
	pub y: u32,
}

impl Point {
	#[inline]
	fn from_raw(p: raw::Point) -> Self {
		Self { x: p.x(), y: p.y() }
	}

	#[inline]
	fn to_raw(&self) -> raw::Point {
		let mut p = raw::Point::default();
		p.set_x(self.x);
		p.set_y(self.y);
		p
	}
}

/// Each component is encoded as the size minus 1, e.g. `16` is encoded as `15`,
/// `65536` is encoded as `65535` (`0xffff`).
#[derive(Clone, Copy, Debug)]
pub struct SizeInclusive {
	pub x: u16,
	pub y: u16,
}

impl SizeInclusive {
	#[inline]
	fn from_raw(p: raw::SizeInclusive) -> Self {
		Self { x: p.x(), y: p.y() }
	}

	#[inline]
	fn to_raw(&self) -> raw::SizeInclusive {
		let mut p = raw::SizeInclusive::default();
		p.set_x(self.x);
		p.set_y(self.y);
		p
	}

	#[inline]
	pub fn area(&self) -> usize {
		usize::from(self.x + 1) * usize::from(self.y + 1)
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Flush {
	pub origin: Point,
	pub size: SizeInclusive,
}

impl Flush {
	#[inline]
	pub fn decode(raw: [u8; 12]) -> Self {
		let f = raw::Flush::from_raw(&raw, 0);
		Self { origin: Point::from_raw(f.origin()), size: SizeInclusive::from_raw(f.size()) }
	}

	#[inline]
	pub fn encode(self) -> [u8; 12] {
		let mut f = raw::Flush::default();
		f.set_origin(self.origin.to_raw());
		f.set_size(self.size.to_raw());
		let mut r = [0; 12];
		f.to_raw(&mut r, 0);
		r
	}
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
	Resize(Resolution),
}

impl Event {
	#[inline]
	pub fn decode(raw: [u8; 9]) -> Self {
		let e = raw::Event::from_raw(&raw, 0);
		match e.ty() {
			raw::EventType::Resize => Self::Resize(Resolution::from_raw(e.args().resize())),
		}
	}

	#[inline]
	pub fn encode(self) -> [u8; 9] {
		let mut e = raw::Event::default();
		match self {
			Self::Resize(r) => {
				e.set_ty(raw::EventType::Resize);
				let mut a = raw::EventArgs::default();
				a.set_resize(r.to_raw());
				e.set_args(a);
			}
		}
		let mut r = [0; 9];
		e.to_raw(&mut r, 0);
		r
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Resolution {
	pub x: u32,
	pub y: u32,
}

impl Resolution {
	fn from_raw(r: raw::Resolution) -> Self {
		Self { x: r.x(), y: r.y() }
	}

	fn to_raw(&self) -> raw::Resolution {
		let mut e = raw::Resolution::default();
		e.set_x(self.x);
		e.set_y(self.y);
		e
	}

	#[inline]
	pub fn decode(raw: [u8; 8]) -> Self {
		Self::from_raw(raw::Resolution::from_raw(&raw, 0))
	}

	#[inline]
	pub fn encode(self) -> [u8; 8] {
		let mut r = [0; 8];
		self.to_raw().to_raw(&mut r, 0);
		r
	}
}
