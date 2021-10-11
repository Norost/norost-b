use core::cell::UnsafeCell;
use core::ptr;

macro_rules! ety {
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

// TODO how does this interact with Drop?
#[repr(transparent)]
pub struct VolatileCell<T>(UnsafeCell<T>)
where
	T: Copy;

impl<T> VolatileCell<T>
where
	T: Copy
{
	pub fn get(&self) -> T {
		unsafe { ptr::read_volatile(self.0.get()) }
	}

	pub fn set(&self, value: T)  {
		unsafe { ptr::write_volatile(self.0.get(), value) }
	}
}
