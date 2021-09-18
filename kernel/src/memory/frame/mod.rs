//! # Frame allocator
//!
//! For now it's just a big, dumb stack as that's easy.

mod dumb_stack;

use super::Page;
use core::convert::TryInto;
use core::fmt;

/// A Physical Page Number.
///
/// PPNs are guaranteed to be properly aligned and may be optimized for size:
/// * If at most 2^32 pages are expected to be available, PPNS will be 32 bits.
/// * If at most 2^16 pages are expected to be available, PPNS will be 16 bits.
#[cfg(not(any(feature = "mem-max-16t", feature = "mem-max-256m")))]
pub struct PPN(u64);
#[cfg(all(feature = "mem-max-16t", not(feature = "mem-max-256m")))]
pub struct PPN(u32);
#[cfg(feature = "mem-max-256m")]
pub struct PPN(u16);

impl PPN {
	pub fn try_from_usize(ptr: usize) -> Result<Self, PPNError> {
		(ptr % Page::SIZE == 0).then(|| {
			let ptr = ptr / Page::SIZE;
			ptr.try_into().map(Self).map_err(|_| PPNError::OutOfRange)
		}).ok_or(PPNError::Misaligned)?
	}
}

impl fmt::Debug for PPN {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "PPN(0x{:x})", u128::from(self.0) * Page::SIZE as u128)
	}
}

pub enum PPNError {
	Misaligned,
	OutOfRange,
}

/// A single page frame with a variable size.
#[derive(Debug)]
pub struct PageFrame {
	pub base: PPN,
	/// The size of the frame expressed as `2 ^ p2size`.
	pub p2size: u8,
}

/// A region of physical memory
#[derive(Debug)]
pub struct MemoryRegion {
	pub base: PPN,
	pub count: usize,
}

impl MemoryRegion {
	/// Take a single PPN from the memory region.
	pub fn take(&mut self) -> Option<PPN> {
		self.count.checked_sub(1).map(|c| {
			self.base.0 += 1;
			self.count = c;
			PPN(self.base.0 - 1)
		})
	}
}

pub enum AllocateError {
	OutOfFrames,
}

pub enum DeallocateError {}

/// Allocate a range of pages.
///
/// The address hint is used to determine if a hugepage can be allocated and to determine
/// the color.
/// The color hint is used to optimize cache layout by adding an offset to the color.
///
/// The callback will not be called if the requested amount of pages are not available.
pub fn allocate<F>(
	count: usize,
	mut callback: F,
	_hint_address: *const u8,
	_hint_color: u8,
) -> Result<(), AllocateError>
where
	F: FnMut(PageFrame),
{
	let mut stack = dumb_stack::STACK.lock();
	(stack.count() >= count)
		.then(|| {
			for _ in 0..count {
				callback(PageFrame {
					base: stack.pop().unwrap(),
					p2size: 0,
				});
			}
		})
		.ok_or(AllocateError::OutOfFrames)
}

/// Free a range of pages.
///
/// # Safety
///
/// The pages must be allocated and may not be freed multiple times in a row.
pub unsafe fn deallocate<F>(count: usize, mut callback: F) -> Result<(), DeallocateError>
where
	F: FnMut() -> PageFrame,
{
	let mut stack = dumb_stack::STACK.lock();
	for _ in 0..count {
		let mut frame = callback();
		for i in 0..1u16 << frame.p2size {
			let ppn = PPN(frame.base.0 + i);
			stack.push(ppn).expect("stack is full (double free?)");
		}
	}
	Ok(())
}

/// Add a region for allocation
///
/// # Safety
///
/// The region may not already be in use.
pub unsafe fn add_memory_region(mut region: MemoryRegion) {
	let mut stack = dumb_stack::STACK.lock();
	while let Some(ppn) = region.take() {
		// Just discard any leftover pages.
		let _ = stack.push(ppn);
	}
}

/// The amount of free memory in bytes.
pub fn free_memory() -> u128 {
	(dumb_stack::STACK.lock().count() * Page::SIZE).try_into().unwrap()
}
