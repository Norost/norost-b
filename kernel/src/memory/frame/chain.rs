//! Chain of pages with some memory set aside for DMA

use super::PPN;
use core::mem;

pub(super) struct Chain {
	head: Option<PPN>,
	count: usize,
}

impl Chain {
	pub const fn new() -> Self {
		Self {
			head: None,
			count: 0,
		}
	}

	pub fn push(&mut self, ppn: PPN) {
		let prev = mem::replace(&mut self.head, Some(ppn));
		unsafe { ppn.as_ptr().cast::<Option<PPN>>().write(prev) };
		self.count += 1;
	}

	pub fn pop(&mut self) -> Option<PPN> {
		let ppn = self.head?;
		self.head = unsafe { ppn.as_ptr().cast::<Option<PPN>>().read() };
		self.count -= 1;
		Some(ppn)
	}

	pub fn count(&self) -> usize {
		self.count
	}
}
