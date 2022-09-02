//! # Frame allocator
//!
//! For now it's just a big, dumb stack as that's easy.

mod chain;
mod dma;
mod fixed_bitmap;
mod owned;

pub use owned::*;

use {
	super::Page,
	crate::{boot, object_table::Root, sync::SpinLock},
	core::{fmt, num::NonZeroUsize, ptr},
};

/// A Physical Page Number.
///
/// PPNs are guaranteed to be properly aligned and may be optimized for size:
/// * If at most 2^32 pages are expected to be available, PPNS will be 32 bits.
/// * If at most 2^16 pages are expected to be available, PPNS will be 16 bits.
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub struct PPN(pub PPNBox);

#[cfg(not(any(feature = "mem-max-16t", feature = "mem-max-256m")))]
pub type PPNBox = u64;
#[cfg(all(feature = "mem-max-16t", not(feature = "mem-max-256m")))]
pub type PPNBox = u32;
#[cfg(feature = "mem-max-256m")]
pub type PPNBox = u16;

#[cfg(not(any(feature = "mem-max-16t", feature = "mem-max-256m")))]
pub type AtomicPPNBox = core::sync::atomic::AtomicU64;
#[cfg(all(feature = "mem-max-16t", not(feature = "mem-max-256m")))]
pub type AtomicPPNBox = core::sync::atomic::AtomicU32;
#[cfg(feature = "mem-max-256m")]
pub type AtomicPPNBox = core::sync::atomic::AtomicU16;

static DEFAULT: SpinLock<chain::Chain> = SpinLock::new(chain::Chain::new());

impl PPN {
	pub fn try_from_usize(ptr: usize) -> Result<Self, PPNError> {
		(ptr % Page::SIZE == 0)
			.then(|| {
				let ptr = ptr / Page::SIZE;
				ptr.try_into().map(Self).map_err(|_| PPNError::OutOfRange)
			})
			.ok_or(PPNError::Misaligned)?
	}

	pub fn as_ptr(&self) -> *mut Page {
		// SAFETY: PPNs are always in range.
		unsafe {
			let phys = u64::from(self.0) * u64::try_from(Page::SIZE).unwrap();
			super::r#virtual::phys_to_virt(phys).cast()
		}
	}

	/// # Safety
	///
	/// The pointer must be aligned and point to somewhere inside the identity map.
	pub unsafe fn from_ptr(page: *mut Page) -> Self {
		let virt = unsafe { super::r#virtual::virt_to_phys(page.cast()) };
		Self(
			(usize::try_from(virt).unwrap() / Page::SIZE)
				.try_into()
				.unwrap(),
		)
	}

	pub fn next(&self) -> Self {
		Self(self.0 + 1)
	}

	pub fn skip(&self, n: PPNBox) -> Self {
		Self(self.0 + n)
	}

	pub fn as_phys(&self) -> usize {
		self.0 as usize * Page::SIZE
	}

	#[allow(dead_code)]
	fn clear(self) {
		unsafe { ptr::write_bytes(self.as_ptr(), 0, 1) }
	}
}

impl fmt::Debug for PPN {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "PPN(0x{:x})", u128::from(self.0) * Page::SIZE as u128)
	}
}

macro_rules! derive_try_from {
	($ty:ty) => {
		impl TryFrom<PPN> for $ty {
			type Error = OutOfRange;

			fn try_from(ppn: PPN) -> Result<Self, Self::Error> {
				let ob = u32::try_from(Page::OFFSET_BITS).map_err(|_| OutOfRange)?;
				let v = Self::try_from(ppn.0).map_err(|_| OutOfRange)?;
				v.checked_shl(ob).ok_or(OutOfRange)
			}
		}
	};
}

derive_try_from!(u128);
derive_try_from!(u64);
derive_try_from!(u32);
derive_try_from!(u16);
derive_try_from!(u8);
derive_try_from!(usize);

#[derive(Debug)]
pub enum PPNError {
	Misaligned,
	OutOfRange,
}

#[derive(Debug)]
pub struct OutOfRange;

pub struct PageFrameIter {
	pub base: PPN,
	pub count: usize,
}

impl Iterator for PageFrameIter {
	type Item = PPN;

	fn next(&mut self) -> Option<Self::Item> {
		self.count.checked_sub(1).map(|c| {
			let b = self.base;
			self.base = self.base.next();
			self.count = c;
			b
		})
	}

	fn count(self) -> usize {
		self.count
	}
}

impl ExactSizeIterator for PageFrameIter {
	fn len(&self) -> usize {
		self.count
	}
}

#[derive(Debug)]
pub enum AllocateError {
	OutOfFrames,
}

#[derive(Debug)]
pub enum DeallocateError {}

pub struct AllocateHints {
	pub address: *const u8,
	pub color: u8,
}

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
	F: FnMut(PPN),
{
	let mut stack = DEFAULT.auto_lock();
	(stack.count() >= count)
		.then(|| {
			for _ in 0..count {
				callback(stack.pop().unwrap());
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
	F: FnMut() -> PPN,
{
	let mut stack = DEFAULT.auto_lock();
	for _ in 0..count {
		stack.push(callback())
	}
	Ok(())
}

#[allow(dead_code)]
pub fn free_memory() -> usize {
	DEFAULT.lock().count() * 4096
}

/// # Safety
///
/// This may only be called once at boot time.
pub(super) unsafe fn init(memory_regions: &mut [boot::MemoryRegion]) {
	unsafe { dma::init(memory_regions) }
	let mut stack = DEFAULT.isr_lock();
	for mr in memory_regions.iter_mut() {
		while let Some(addr) = mr.take_page() {
			if let Ok(ppn) = (addr >> Page::OFFSET_BITS).try_into() {
				stack.push(PPN(ppn));
			}
		}
	}
}

/// # Safety
///
/// This may only be called once at boot time.
pub(super) fn post_init(root: &Root) {
	dma::post_init(root)
}
