use core::{cell::Cell, num::NonZeroUsize};

#[repr(C)]
pub struct TSS {
	_reserved_0: [u32; 1],
	rsp: [[Cell<u32>; 2]; 3],
	_reserved_1: [u32; 2],
	ist: [[Cell<u32>; 2]; 7],
	_reserved_2: [u32; 2],
	_reserved_3: u16,
	iomap_base: Cell<u16>,
}

impl TSS {
	pub const fn new() -> Self {
		Self {
			_reserved_0: [0; 1],
			rsp: [const { [Cell::new(0), Cell::new(0)] }; 3],
			_reserved_1: [0; 2],
			ist: [const { [Cell::new(0), Cell::new(0)] }; 7],
			_reserved_2: [0; 2],
			_reserved_3: 0,
			iomap_base: Cell::new(0),
		}
	}

	pub unsafe fn set_rsp(&self, rsp: usize, pointer: *const usize) {
		self.rsp[rsp][0].set(pointer as u32);
		self.rsp[rsp][1].set(((pointer as u64) >> 32) as u32);
	}

	pub unsafe fn set_ist(&self, ist: NonZeroUsize, pointer: *const usize) {
		self.ist[ist.get() - 1][0].set(pointer as u32);
		self.ist[ist.get() - 1][1].set(((pointer as u64) >> 32) as u32);
	}
}
