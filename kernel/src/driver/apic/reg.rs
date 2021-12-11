use core::cell::UnsafeCell;

#[repr(C)]
pub struct RegR {
	value: UnsafeCell<u32>,
	_dont_touch: [UnsafeCell<u32>; 3],
}

impl RegR {
	pub fn get(&self) -> u32 {
		unsafe { core::ptr::read_volatile(self.value.get()) }
	}
}

#[repr(C)]
pub struct RegW {
	value: UnsafeCell<u32>,
	_dont_touch: [UnsafeCell<u32>; 3],
}

impl RegW {
	pub fn set(&self, value: u32) {
		unsafe { core::ptr::write_volatile(self.value.get(), value) }
	}
}

#[repr(C)]
pub struct RegRW {
	value: UnsafeCell<u32>,
	_dont_touch: [UnsafeCell<u32>; 3],
}

impl RegRW {
	pub fn get(&self) -> u32 {
		unsafe { core::ptr::read_volatile(self.value.get()) }
	}

	pub fn set(&self, value: u32) {
		unsafe { core::ptr::write_volatile(self.value.get(), value) }
	}
}
