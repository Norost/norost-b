use crate::Handle;
use core::ops::RangeInclusive;

macro_rules! impl_ {
	{ $($v:ident $i:literal)* } => {
		#[derive(Clone, Copy)]
		pub enum NewObjectType {
			$($v = $i,)*
		}

		impl NewObjectType {
			pub fn from_raw(n: impl TryInto<u8>) -> Option<Self> {
				Some(match n.try_into().ok()? {
					$($i => Self::$v,)*
					_ => return None,
				})
			}
		}
	};
}

impl_! {
	SubRange 0
	Root 1
	Duplicate 2
	SharedMemory 3
}

pub enum NewObject {
	SubRange {
		handle: Handle,
		range: RangeInclusive<usize>,
	},
	Root,
	Duplicate {
		handle: Handle,
	},
	SharedMemory {
		size: usize,
	},
}

pub enum NewObjectArgs {
	N0,
	N1(usize),
	N2(usize, usize),
	N3(usize, usize, usize),
}

impl NewObject {
	#[inline]
	pub fn into_args(self) -> (usize, NewObjectArgs) {
		use NewObjectArgs::*;
		use NewObjectType::*;
		let (t, a) = match self {
			Self::SubRange { handle, range } => {
				(SubRange, N3(handle as _, *range.start(), *range.end()))
			}
			Self::Root => (Root, N0),
			Self::Duplicate { handle } => (Duplicate, N1(handle as _)),
			Self::SharedMemory { size } => (SharedMemory, N1(size)),
		};
		(t as _, a)
	}

	#[inline]
	pub fn try_from_args(ty: usize, a: usize, b: usize, c: usize) -> Option<Self> {
		use NewObjectType::*;
		Some(match NewObjectType::from_raw(ty)? {
			SubRange => Self::SubRange {
				handle: a as _,
				range: b..=c,
			},
			Root => Self::Root,
			Duplicate => Self::Duplicate { handle: a as _ },
			SharedMemory => Self::SharedMemory { size: a },
		})
	}
}
