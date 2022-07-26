//! Traits & types useful for completion-based asynchronous interfaces.
//!
//! Based on `tokio_uring`.

#![no_std]
#![deny(unused)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{boxed::Box, rc::Rc, sync::Arc, vec::Vec};
use core::ops::{Bound, Range, RangeBounds};

pub unsafe trait Buf: Unpin + 'static {
	fn as_ptr(&self) -> *const u8;

	fn bytes_init(&self) -> usize;

	fn bytes_total(&self) -> usize;

	// track_caller has a lot of overhead, so only enable in debug mode.
	#[cfg_attr(debug_assertions, track_caller)]
	fn slice(self, range: impl RangeBounds<usize>) -> Slice<Self>
	where
		Self: Sized,
	{
		// Try to minimize the amount of panic calls while still producing useful panic output.
		let start = match range.start_bound() {
			Bound::Included(&s) => Some(s),
			Bound::Excluded(&s) => s.checked_add(1),
			Bound::Unbounded => Some(0),
		};
		let total = self.bytes_total();
		let end = match range.end_bound() {
			Bound::Included(&s) => s.checked_add(1),
			Bound::Excluded(&s) => Some(s),
			Bound::Unbounded => Some(total),
		};
		let start = start.and_then(|s| (s <= total).then(|| s));
		let end = end.and_then(|s| (s <= total).then(|| s));
		let range = start
			.and_then(|s| end.map(|e| s..e))
			.expect("invalid range");
		assert!(
			range.start <= self.bytes_init(),
			"start bound outside initialized memory"
		);
		assert!(range.end <= total, "end bound outside total memory");
		Slice { buf: self, range }
	}
}

pub unsafe trait BufMut: Buf {
	fn as_mut_ptr(&mut self) -> *mut u8;

	unsafe fn set_bytes_init(&mut self, n: usize);
}

pub struct Slice<B: Buf> {
	buf: B,
	range: Range<usize>,
}

impl<B: Buf> Slice<B> {
	pub fn range(&self) -> Range<usize> {
		self.range.clone()
	}

	pub fn into_inner(self) -> B {
		self.buf
	}
}

unsafe impl<B: Buf> Buf for Slice<B> {
	fn as_ptr(&self) -> *const u8 {
		// SAFETY: we ensured beforehand the range is valid.
		unsafe { self.buf.as_ptr().add(self.range.start) }
	}

	fn bytes_init(&self) -> usize {
		(self.buf.bytes_init() - self.range.start).min(self.bytes_total())
	}

	fn bytes_total(&self) -> usize {
		self.range.len()
	}
}

unsafe impl<B: BufMut> BufMut for Slice<B> {
	fn as_mut_ptr(&mut self) -> *mut u8 {
		self.as_ptr() as *mut _
	}

	unsafe fn set_bytes_init(&mut self, n: usize) {
		unsafe { self.buf.set_bytes_init(self.range.start + n) }
	}
}

#[cfg(feature = "alloc")]
unsafe impl Buf for Vec<u8> {
	fn as_ptr(&self) -> *const u8 {
		self.as_ptr()
	}

	fn bytes_init(&self) -> usize {
		self.len()
	}

	fn bytes_total(&self) -> usize {
		self.capacity()
	}
}

#[cfg(feature = "alloc")]
unsafe impl BufMut for Vec<u8> {
	fn as_mut_ptr(&mut self) -> *mut u8 {
		self.as_mut_ptr()
	}

	unsafe fn set_bytes_init(&mut self, n: usize) {
		unsafe { self.set_len(n) }
	}
}

macro_rules! owned_slice {
	($ty:ident) => {
		#[cfg(feature = "alloc")]
		unsafe impl Buf for $ty<[u8]> {
			fn as_ptr(&self) -> *const u8 {
				(**self).as_ptr()
			}

			fn bytes_init(&self) -> usize {
				self.len()
			}

			fn bytes_total(&self) -> usize {
				self.len()
			}
		}
	};
}

owned_slice!(Box);
owned_slice!(Rc);
owned_slice!(Arc);

unsafe impl Buf for &'static [u8] {
	fn as_ptr(&self) -> *const u8 {
		(*self).as_ptr()
	}

	fn bytes_init(&self) -> usize {
		self.len()
	}

	fn bytes_total(&self) -> usize {
		self.len()
	}
}

unsafe impl<const N: usize> Buf for &'static [u8; N] {
	fn as_ptr(&self) -> *const u8 {
		*self as _
	}

	fn bytes_init(&self) -> usize {
		self.len()
	}

	fn bytes_total(&self) -> usize {
		self.len()
	}
}

unsafe impl Buf for &'static str {
	fn as_ptr(&self) -> *const u8 {
		(*self).as_ptr()
	}

	fn bytes_init(&self) -> usize {
		self.len()
	}

	fn bytes_total(&self) -> usize {
		self.len()
	}
}

unsafe impl Buf for () {
	fn as_ptr(&self) -> *const u8 {
		1 as _
	}

	fn bytes_init(&self) -> usize {
		0
	}

	fn bytes_total(&self) -> usize {
		0
	}
}

unsafe impl BufMut for () {
	fn as_mut_ptr(&mut self) -> *mut u8 {
		1 as _
	}

	unsafe fn set_bytes_init(&mut self, _: usize) {}
}
