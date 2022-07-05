use crate::{Handle, RWX};
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
	StreamTable 4
	PermissionMask 5
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pow2Size(pub u8);

macro_rules! p2s {
	($t:ident) => {
		impl TryFrom<$t> for Pow2Size {
			type Error = ();

			#[inline]
			fn try_from(n: $t) -> Result<Self, Self::Error> {
				(n.count_ones() == 1)
					.then(|| Self(n.trailing_zeros().try_into().unwrap()))
					.ok_or(())
			}
		}

		impl TryFrom<Pow2Size> for $t {
			type Error = ();

			#[inline]
			fn try_from(n: Pow2Size) -> Result<Self, Self::Error> {
				(usize::from(n.0) < core::mem::size_of::<$t>() * 8)
					.then(|| (1 << n.0))
					.ok_or(())
			}
		}
	};
	(s $t:ident) => {
		impl TryFrom<$t> for Pow2Size {
			type Error = ();

			#[inline]
			fn try_from(n: $t) -> Result<Self, Self::Error> {
				(n > 0 && n.count_ones() == 1)
					.then(|| Self(n.trailing_zeros().try_into().unwrap()))
					.ok_or(())
			}
		}

		impl TryFrom<Pow2Size> for $t {
			type Error = ();

			#[inline]
			fn try_from(n: Pow2Size) -> Result<Self, Self::Error> {
				(usize::from(n.0) < core::mem::size_of::<$t>() * 8 - 1)
					.then(|| (1 << n.0))
					.ok_or(())
			}
		}
	};
}

p2s!(u8);
p2s!(u16);
p2s!(u32);
p2s!(u64);
p2s!(u128);
p2s!(usize);
p2s!(s i8);
p2s!(s i16);
p2s!(s i32);
p2s!(s i64);
p2s!(s i128);
p2s!(s isize);

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
	StreamTable {
		buffer_mem: Handle,
		buffer_mem_block_size: Pow2Size,
		allow_sharing: bool,
	},
	PermissionMask {
		handle: Handle,
		rwx: RWX,
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
			Self::StreamTable {
				buffer_mem,
				buffer_mem_block_size,
				allow_sharing,
			} => (
				StreamTable,
				N2(
					buffer_mem as _,
					usize::from(buffer_mem_block_size.0) | usize::from(allow_sharing) << 8,
				),
			),
			Self::PermissionMask { handle, rwx } => {
				(PermissionMask, N2(handle as _, rwx.into_raw() as _))
			}
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
			StreamTable => Self::StreamTable {
				buffer_mem: a as _,
				buffer_mem_block_size: Pow2Size(b as _),
				allow_sharing: b & (1 << 8) != 0,
			},
			PermissionMask => Self::PermissionMask {
				handle: a as _,
				rwx: RWX::try_from_raw((b & 7) as u8)?,
			},
		})
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn pow2size() {
		let n = 1 << 20;
		let v = Pow2Size::try_from(n).unwrap();
		let v = u32::try_from(v).unwrap();
		assert_eq!(v, n);
	}

	#[test]
	fn pow2size_inval() {
		let n = 1 << 20 | 1 << 11;
		assert!(Pow2Size::try_from(n).is_err());
	}
}
