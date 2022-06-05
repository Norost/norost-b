extern crate alloc;

use crate::Handle;
use alloc::vec::Vec;
use norostb_rt::{io, Error};

pub enum Job<'a> {
	Read {
		job_id: u32,
		handle: Handle,
		length: u64,
	},
	Peek {
		job_id: u32,
		handle: Handle,
		length: u64,
	},
	Write {
		job_id: u32,
		handle: Handle,
		data: &'a [u8],
	},
	Open {
		job_id: u32,
		handle: Handle,
		path: &'a [u8],
	},
	Create {
		job_id: u32,
		handle: Handle,
		path: &'a [u8],
	},
	Close {
		handle: Handle,
	},
	Seek {
		job_id: u32,
		handle: Handle,
		from: io::SeekFrom,
	},
}

macro_rules! with {
	(@clear $fn:ident -> $fnc:ident ($a:ident : $t:ty)) => {
		#[doc = concat!("Same as ", stringify!($fn), " but clears the buffer first")]
		pub fn $fnc(mut buf: Vec<u8>, job_id: u32, $a: $t) -> Result<Vec<u8>, (Vec<u8>, ())> {
			buf.clear();
			match Self::$fn(&mut buf, job_id, $a) {
				Ok(()) => Ok(buf),
				Err(e) => Err((buf, e)),
			}
		}
	};
	(handle $fn:ident $fnc:ident = $ty:ident) => {
		pub fn $fn(buf: &mut Vec<u8>, job_id: u32, handle: Handle) -> Result<(), ()> {
			buf.extend_from_slice(
				io::Job {
					ty: io::Job::$ty,
					job_id,
					handle,
					..Default::default()
				}
				.as_ref(),
			);
			Ok(())
		}
		with!(@clear $fn -> $fnc(handle: Handle));
	};
	(buf $fn:ident $fnc:ident = $ty:ident) => {
		pub fn $fn<F>(buf: &mut Vec<u8>, job_id: u32, data: F) -> Result<(), ()>
		where
			F: FnOnce(&mut Vec<u8>) -> Result<(), ()>,
		{
			buf.extend_from_slice(
				io::Job {
					ty: io::Job::$ty,
					job_id,
					..Default::default()
				}
				.as_ref(),
			);
			data(buf)
		}
		#[doc = concat!("Same as ", stringify!($fn), " but clears the buffer first")]
		pub fn $fnc<F>(mut buf: Vec<u8>, job_id: u32, data: F) -> Result<Vec<u8>, (Vec<u8>, ())>
		where
			F: FnOnce(&mut Vec<u8>) -> Result<(), ()>,
		{
			buf.clear();
			match Self::$fn(&mut buf, job_id, data) {
				Ok(()) => Ok(buf),
				Err(e) => Err((buf, e)),
			}
		}
	};
	(u64 $fn:ident $fnc:ident = $ty:ident, $f:ident) => {
		pub fn $fn(buf: &mut Vec<u8>, job_id: u32, $f: u64) -> Result<(), ()> {
			buf.extend_from_slice(
				io::Job {
					ty: io::Job::$ty,
					job_id,
					..Default::default()
				}
				.as_ref(),
			);
			buf.extend_from_slice(&$f.to_ne_bytes());
			Ok(())
		}
		with!(@clear $fn -> $fnc($f: u64));
	};
}

macro_rules! is {
	($f:ident = $v:ident) => {
		pub fn $f(&self) -> bool {
			match self {
				Self::$v { .. } => true,
				_ => false,
			}
		}
	};
}

impl<'a> Job<'a> {
	pub fn deserialize(data: &'a [u8]) -> Option<Self> {
		let (job, data) = io::Job::deserialize(data)?;
		let (job_id, handle) = (job.job_id, job.handle);
		Some(match job.ty {
			io::Job::READ => Self::Read {
				job_id,
				handle,
				length: u64::from_ne_bytes(data.try_into().ok()?),
			},
			io::Job::PEEK => Self::Peek {
				job_id,
				handle,
				length: u64::from_ne_bytes(data.try_into().ok()?),
			},
			io::Job::WRITE => Self::Write {
				job_id,
				handle,
				data,
			},
			io::Job::OPEN => Self::Open {
				job_id,
				handle,
				path: data,
			},
			io::Job::CREATE => Self::Create {
				job_id,
				handle,
				path: data,
			},
			io::Job::CLOSE => Self::Close { handle },
			io::Job::SEEK => {
				let offt = u64::from_ne_bytes(data.try_into().ok()?);
				Self::Seek {
					job_id,
					handle,
					from: io::SeekFrom::try_from_raw(job.from_anchor, offt).ok()?,
				}
			}
			_ => return None,
		})
	}

	with!(buf reply_read reply_read_clear = READ);
	with!(buf reply_peek reply_peek_clear = PEEK);
	with!(handle reply_open reply_open_clear = OPEN);
	with!(handle reply_create reply_create_clear = CREATE);
	with!(u64 reply_write reply_write_clear = WRITE, amount);
	with!(u64 reply_seek reply_seek_clear = SEEK, position);

	pub fn reply_error(buf: &mut Vec<u8>, job_id: u32, error: Error) -> Result<(), ()> {
		buf.extend(
			io::Job {
				job_id,
				result: error as _,
				..Default::default()
			}
			.as_ref(),
		);
		Ok(())
	}

	/// Same as [`Self::reply_error`] but clears the buffer first.
	pub fn reply_error_clear(
		mut buf: Vec<u8>,
		job_id: u32,
		error: Error,
	) -> Result<Vec<u8>, (Vec<u8>, ())> {
		buf.clear();
		match Self::reply_error(&mut buf, job_id, error) {
			Ok(()) => Ok(buf),
			Err(e) => Err((buf, e)),
		}
	}

	is!(is_read = Read);
	is!(is_peek = Peek);
	is!(is_write = Write);
	is!(is_open = Open);
	is!(is_create = Create);
	is!(is_close = Close);
	is!(is_seek = Seek);
}
