#![no_std]

use core::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign};

macro_rules! ety {
	(@INTERNAL impl arithemic $name:ident, $ty:ty, $trait:ident.$fn:ident) => {
		impl $trait<Self> for $name {
			type Output = Self;

			fn $fn(self, rhs: Self) -> Self {
				$ty::from(self.0).$fn($ty::from(rhs.0)).into()
			}
		}
	};
	(@INTERNAL impl bitwise $name:ident, $ty:ty, $trait:ident.$fn:ident, $traitas:ident.$fnas:ident) => {
		impl $trait<Self> for $name {
			type Output = Self;

			fn $fn(self, rhs: Self) -> Self {
				Self(self.0.$fn(rhs.0))
			}
		}

		impl $traitas<Self> for $name {
			fn $fnas(&mut self, rhs: Self) {
				self.0 = self.0.$fn(rhs.0)
			}
		}
	};
	(@INTERNAL $ty:ty, $name:ident, $from:ident, $to:ident) => {
		#[allow(non_camel_case_types)]
		#[derive(Clone, Copy, PartialEq, Eq)]
		#[repr(transparent)]
		pub struct $name($ty);

		impl From<$ty> for $name {
			fn from(value: $ty) -> Self {
				Self(value.$to())
			}
		}

		impl From<$name> for $ty {
			fn from(value: $name) -> Self {
				Self::$from(value.0)
			}
		}

		ety!(@INTERNAL impl bitwise $name, $ty, BitOr.bitor, BitOrAssign.bitor_assign);
		ety!(@INTERNAL impl bitwise $name, $ty, BitAnd.bitand, BitAndAssign.bitand_assign);
		ety!(@INTERNAL impl bitwise $name, $ty, BitXor.bitxor, BitXorAssign.bitxor_assign);
	};
	(be $ty:ty, $name:ident) => {
		ety!(@INTERNAL $ty, $name, from_be, to_be);
	};
	(le $ty:ty, $name:ident) => {
		ety!(@INTERNAL $ty, $name, from_le, to_le);
	};
}

ety!(be u8, u8be);
ety!(be u16, u16be);
ety!(be u32, u32be);
ety!(be u64, u64be);
ety!(le u8, u8le);
ety!(le u16, u16le);
ety!(le u32, u32le);
ety!(le u64, u64le);
