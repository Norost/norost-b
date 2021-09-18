//! # Dumb stack-based frame allocator

use super::*;
use crate::sync::SpinLock;
use core::mem::MaybeUninit;

/// 64K * 4K = 256MiB. Not a lot but enough for now.
pub static STACK: SpinLock<Stack> = SpinLock::new(Stack {
	stack: MaybeUninit::uninit_array(),
	count: 0,
});

pub struct Stack {
	stack: [MaybeUninit<PPN>; 65536],
	count: usize,
}

impl Stack {
	pub fn push(&mut self, ppn: PPN) -> Result<(), Full> {
		(self.count < self.stack.len())
			.then(|| {
				self.stack[self.count].write(ppn);
				self.count += 1;
			})
			.ok_or(Full)
	}

	pub fn pop(&mut self) -> Option<PPN> {
		self.count.checked_sub(1).map(|c| {
			self.count = c;
			unsafe { self.stack[c].assume_init_read() }
		})
	}

	pub fn count(&self) -> usize {
		self.count
	}
}

#[derive(Debug)]
pub struct Full;
