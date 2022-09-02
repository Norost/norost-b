//! Chain of pages with some memory set aside for DMA

use {super::PPN, core::mem};

pub(super) struct Chain {
	head: PPN,
	count: usize,
}

impl Chain {
	pub const fn new() -> Self {
		Self { head: PPN(0), count: 0 }
	}

	pub fn push(&mut self, ppn: PPN) {
		let prev = mem::replace(&mut self.head, ppn);
		// For debugging use-after-frees
		#[cfg(debug_assertions)]
		unsafe {
			ppn.as_ptr().cast::<u8>().write_bytes(0x69, 4096)
		};
		unsafe { ppn.as_ptr().cast::<PPN>().write(prev) };
		self.count += 1;
	}

	pub fn pop(&mut self) -> Option<PPN> {
		self.count.checked_sub(1).map(|c| {
			self.count = c;
			let next = unsafe { self.head.as_ptr().cast::<PPN>().read() };
			mem::replace(&mut self.head, next)
		})
	}

	pub fn count(&self) -> usize {
		self.count
	}
}
