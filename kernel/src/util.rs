//! # Various useful utilities that probably exist in a crate somewhere but meh.
#![allow(dead_code)]

use core::{
	cell::Cell,
	fmt::{self, Write},
	str,
};
use norostb_kernel::Handle;

/// Pretty print an iterator.
///
/// # Note
///
/// This consumes the iterator.
pub struct DebugIter<I: Iterator<Item = T>, T>(Cell<Option<I>>);

impl<I: Iterator<Item = T>, T> DebugIter<I, T> {
	pub fn new(iter: I) -> Self {
		Self(Cell::new(Some(iter)))
	}
}

impl<I: Iterator<Item = T>, T: fmt::Debug> fmt::Debug for DebugIter<I, T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entries(self.0.take().unwrap()).finish()
	}
}

/// Lossy UTF-8 printer for byte slices without allocations.
pub struct ByteStr<'a>(&'a [u8]);

impl<'a> ByteStr<'a> {
	pub fn new(s: &'a [u8]) -> Self {
		Self(s)
	}
}

impl fmt::Display for ByteStr<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut s = self.0;
		loop {
			match str::from_utf8(s) {
				Ok(s) => break f.write_str(s),
				Err(e) => {
					f.write_str(str::from_utf8(&s[..e.valid_up_to()]).unwrap())?;
					f.write_char('\u{fffe}')?;
					s = &s[e.valid_up_to() + 1..];
				}
			}
		}
	}
}

impl fmt::Debug for ByteStr<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_char('"')?;
		let mut s = self.0;
		loop {
			match str::from_utf8(s) {
				Ok(s) => {
					s.escape_debug().try_for_each(|c| f.write_char(c))?;
					break;
				}
				Err(e) => {
					str::from_utf8(&s[..e.valid_up_to()])
						.unwrap()
						.escape_debug()
						.try_for_each(|c| f.write_char(c))?;
					f.write_char('\u{fffe}')?;
					s = &s[e.valid_up_to() + 1..];
				}
			}
		}
		f.write_char('"')
	}
}

/// Converts a typed [`arena::Arena`] handle to a generic [`Handle`] suitable for FFI.
#[cfg_attr(debug_assertions, track_caller)]
#[inline]
pub fn erase_handle(handle: arena::Handle<u8>) -> Handle {
	let (index, generation) = handle.into_raw();
	assert!(index < 1 << 24, "can't construct unique handle");
	(generation as u32) << 24 | index as u32
}

/// Converts an untyped [`Handle`] to an [`arena::Arena`] handle.
#[cfg_attr(debug_assertions, track_caller)]
#[inline]
pub fn unerase_handle(handle: Handle) -> arena::Handle<u8> {
	arena::Handle::from_raw(
		(handle & 0xff_ffff).try_into().unwrap(),
		(handle >> 24) as u8,
	)
}
