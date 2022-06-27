macro_rules! impl_ {
	{ $($v:ident $i:literal)* } => {
		#[derive(Debug)]
		#[non_exhaustive]
		pub enum Error {
			$($v = -$i,)*
		}

		/// Returns [`Ok`] if the value doesn't represent a valid error, [`Err`] otherwise.
		#[inline(always)]
		pub fn result<T: raw::RawError>(value: T) -> Result<T> {
			Err(match value.to_i64() {
				$(-$i => Error::$v,)*
				-4096..=-1 => Error::Unknown,
				_ => return Ok(value),
			})
		}
	};
}

impl_! {
	Unknown 1
	DoesNotExist 2
	AlreadyExists 3
	InvalidOperation 4
	Cancelled 5
	CantCreateObject 6
	InvalidObject 7
	InvalidData 8
}

impl<T: raw::RawError> From<T> for Error {
	fn from(t: T) -> Error {
		match result(t) {
			Ok(_) => Error::Unknown,
			Err(e) => e,
		}
	}
}

pub type Result<T> = core::result::Result<T, Error>;

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
