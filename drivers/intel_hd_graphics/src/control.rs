use core::ptr::{self, NonNull};

pub struct Control {
	base: NonNull<u8>,
}

impl Control {
	pub fn new(base: NonNull<u8>) -> Self {
		assert_eq!(base.as_ptr() as usize & 3, 0, "bad alignment");
		Self { base }
	}

	pub unsafe fn load(&mut self, offset: u32) -> u32 {
		ptr::read_volatile(self.base.as_ptr().add(offset.try_into().unwrap()).cast())
	}

	pub unsafe fn store(&mut self, offset: u32, value: u32) {
		ptr::write_volatile(
			self.base.as_ptr().add(offset.try_into().unwrap()).cast(),
			value,
		)
	}

	pub unsafe fn load_byte(&mut self, offset: u32) -> u8 {
		ptr::read_volatile(self.base.as_ptr().add(offset.try_into().unwrap()).cast())
	}

	pub unsafe fn store_byte(&mut self, offset: u32, value: u8) {
		ptr::write_volatile(
			self.base.as_ptr().add(offset.try_into().unwrap()).cast(),
			value,
		)
	}
}
