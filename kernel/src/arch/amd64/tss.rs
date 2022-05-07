use core::num::NonZeroUsize;

#[repr(C)]
pub struct TSS {
	_reserved_0: [u32; 1],
	rsp: [[u32; 2]; 3],
	_reserved_1: [u32; 2],
	ist: [[u32; 2]; 7],
	_reserved_2: [u32; 2],
	_reserved_3: u16,
	iomap_base: u16,
}

impl TSS {
	pub const fn new() -> Self {
		Self {
			_reserved_0: [0; 1],
			rsp: [[0; 2]; 3],
			_reserved_1: [0; 2],
			ist: [[0; 2]; 7],
			_reserved_2: [0; 2],
			_reserved_3: 0,
			iomap_base: 0,
		}
	}

	pub unsafe fn set_rsp(&mut self, rsp: usize, pointer: *const usize) {
		self.rsp[rsp][0] = pointer as u32;
		self.rsp[rsp][1] = ((pointer as u64) >> 32) as u32;
	}

	pub unsafe fn set_ist(&mut self, ist: NonZeroUsize, pointer: *const usize) {
		self.ist[ist.get() - 1][0] = pointer as u32;
		self.ist[ist.get() - 1][1] = ((pointer as u64) >> 32) as u32;
	}
}
