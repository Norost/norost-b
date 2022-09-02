#![no_std]
use core::{cell::UnsafeCell, ptr};

// TODO how does this interact with Drop?
#[repr(transparent)]
pub struct VolatileCell<T>(UnsafeCell<T>)
where
	T: Copy;

impl<T> VolatileCell<T>
where
	T: Copy,
{
	pub fn get(&self) -> T {
		unsafe { ptr::read_volatile(self.0.get()) }
	}

	pub fn set(&self, value: T) {
		unsafe { ptr::write_volatile(self.0.get(), value) }
	}
}
