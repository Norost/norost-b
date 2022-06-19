#![no_std]

use core::{fmt, ops::*};

macro_rules! ety {
	($name:ident, $ty:ty, $trait:ident.$fn:ident, $traitas:ident.$fnas:ident) => {
		impl $trait<Self> for $name {
			type Output = Self;

			fn $fn(self, rhs: Self) -> Self {
				Self::from(<$ty>::from(self).$fn(<$ty>::from(rhs)))
			}
		}

		impl $trait<$ty> for $name {
			type Output = Self;

			fn $fn(self, rhs: $ty) -> Self {
				Self::from(<$ty>::from(self).$fn(rhs))
			}
		}

		impl $trait<$name> for $ty {
			type Output = Self;

			fn $fn(self, rhs: $name) -> Self {
				self.$fn(Self::from(rhs))
			}
		}

		impl $traitas<Self> for $name {
			fn $fnas(&mut self, rhs: Self) {
				*self = self.$fn(rhs)
			}
		}

		impl $traitas<$ty> for $name {
			fn $fnas(&mut self, rhs: $ty) {
				*self = self.$fn(rhs)
			}
		}

		impl $traitas<$name> for $ty {
			fn $fnas(&mut self, rhs: $name) {
				*self = self.$fn(rhs)
			}
		}
	};
	($ty:ty, $name:ident, $from:ident, $to:ident) => {
		#[allow(non_camel_case_types)]
		#[derive(Clone, Copy, Default, PartialEq, Eq)]
		#[repr(transparent)]
		pub struct $name($ty);

		impl $name {
			pub const fn new(value: $ty) -> Self {
				Self(value.$to())
			}
		}

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

		impl fmt::Debug for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				<$ty>::from(self.0).fmt(f)
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				<$ty>::from(self.0).fmt(f)
			}
		}

		impl fmt::LowerHex for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				<$ty>::from(self.0).fmt(f)
			}
		}

		impl fmt::UpperHex for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				<$ty>::from(self.0).fmt(f)
			}
		}

		ety!($name, $ty, Add.add, AddAssign.add_assign);
		ety!($name, $ty, Sub.sub, SubAssign.sub_assign);
		ety!($name, $ty, Mul.mul, MulAssign.mul_assign);
		ety!($name, $ty, Div.div, DivAssign.div_assign);
		ety!($name, $ty, Rem.rem, RemAssign.rem_assign);
		ety!($name, $ty, BitOr.bitor, BitOrAssign.bitor_assign);
		ety!($name, $ty, BitAnd.bitand, BitAndAssign.bitand_assign);
		ety!($name, $ty, BitXor.bitxor, BitXorAssign.bitxor_assign);

		impl Not for $name {
			type Output = Self;

			fn not(self) -> Self {
				Self(self.0.not())
			}
		}
	};
	(be $ty:ty, $name:ident) => {
		ety!($ty, $name, from_be, to_be);
	};
	(le $ty:ty, $name:ident) => {
		ety!($ty, $name, from_le, to_le);
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

#[cfg(test)]
mod test {
	use super::*;

	// Just one test is plenty to ensure we've implemented the operators correctly.
	#[test]
	fn add() {
		assert_eq!(u32le::from(5) + u32le::from(7), 12.into());
		assert_eq!(u32le::from(5) + 7, 12.into());
		assert_eq!(5 + u32le::from(7), 12);
	}
}
