#[derive(Debug)]
pub enum Error {
	Unknown,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Returns [`Ok`] if the value doesn't represent a valid error, [`Err`] otherwise.
#[inline(always)]
pub fn result<T: raw::RawError>(value: T) -> Result<T> {
	match value.to_i64() {
		-4096..=-1 => Err(Error::Unknown),
		_ => Ok(value),
	}
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

	impl RawError for i64 {
		fn to_i64(&self) -> i64 {
			*self
		}
	}
}
