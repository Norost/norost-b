#[derive(Debug)]
pub enum Error {
	Unknown = -1,
	DoesNotExist = -2,
	AlreadyExists = -3,
	InvalidOperation = -4,
	Cancelled = -5,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Returns [`Ok`] if the value doesn't represent a valid error, [`Err`] otherwise.
#[inline(always)]
pub fn result<T: raw::RawError>(value: T) -> Result<T> {
	Err(match value.to_i64() {
		// SAFETY: the value is a valid Error variant
		e @ -5..=-1 => unsafe { core::mem::transmute(e as i8) },
		-4096..=-1 => Error::Unknown,
		_ => return Ok(value),
	})
}

#[doc(hidden)]
mod raw {
	pub trait RawError {
		fn to_i64(&self) -> i64;
	}

	impl RawError for isize {
		fn to_i64(&self) -> i64 {
			*self as i64
		}
	}

	impl RawError for i16 {
		fn to_i64(&self) -> i64 {
			(*self).into()
		}
	}

	impl RawError for i32 {
		fn to_i64(&self) -> i64 {
			(*self).into()
		}
	}

	impl RawError for i64 {
		fn to_i64(&self) -> i64 {
			*self
		}
	}
}
