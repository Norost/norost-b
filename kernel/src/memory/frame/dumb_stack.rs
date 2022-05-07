//! # Dumb stack-based frame allocator

use super::*;
use crate::sync::SpinLock;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::slice::SliceIndex;

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

	/// Pop a contiguous range of page. Useful for DMA.
	///
	/// Returns the start address of the range.
	pub fn pop_contiguous_range(&mut self, count: NonZeroUsize) -> Option<PPN> {
		if self.count == 0 {
			return None;
		}

		let s = self.get_mut(..).unwrap();

		// Sort so we can easily find a sufficiently large range.
		// The depth is limited for now to avoid overflowing the tiny, weenie 4KB stack.
		s[..128].sort_unstable();

		// Find the smallest range to split.
		let mut best: Option<(PPN, NonZeroUsize, usize)> = None;
		let mut candidate = (s[0], NonZeroUsize::new(1).unwrap(), 0);
		for (i, &n) in s[..128].iter().enumerate().skip(1) {
			// TODO don't unwrap
			if candidate.0.skip(candidate.1.get().try_into().unwrap()) != n {
				if candidate.1 >= count && best.map_or(true, |b| b.1 > candidate.1) {
					best = Some(candidate);
				}
				candidate = (n, NonZeroUsize::new(1).unwrap(), i);
			} else {
				candidate.1 = NonZeroUsize::new(candidate.1.get() + 1).unwrap();
			}
		}
		let best = best.or_else(|| (candidate.1 >= count).then(|| candidate));

		// Remove the best range from the list, if any
		if let Some((base, _, index)) = best {
			s.copy_within(index + count.get().., index);
			self.count -= count.get();
			Some(base)
		} else {
			None
		}
	}

	pub fn count(&self) -> usize {
		self.count
	}

	fn get_mut<I>(&mut self, index: I) -> Option<&mut [PPN]>
	where
		I: SliceIndex<[PPN], Output = [PPN]>,
	{
		// SAFETY: all elements up to count are initialized.
		let s = unsafe { MaybeUninit::slice_assume_init_mut(&mut self.stack[..self.count]) };
		index.get_mut(s)
	}
}

#[derive(Debug)]
pub struct Full;
