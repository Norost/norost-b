//! # Various useful utilities that probably exist in a crate somewhere but meh.

use core::{
	cell::Cell,
	fmt::{self, Write},
	str,
};

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
pub struct DebugByteStr<'a>(&'a [u8]);

impl<'a> DebugByteStr<'a> {
	pub fn new(s: &'a [u8]) -> Self {
		Self(s)
	}
}

impl fmt::Debug for DebugByteStr<'_> {
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
					s = &s[e.valid_up_to()..];
				}
			}
		}
		f.write_char('"')
	}
}
