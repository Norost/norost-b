#![no_std]

use core::mem::MaybeUninit;
use norostb_kernel::{io, syscall};

pub use norostb_kernel::{
	error,
	io::{Handle, Response, SeekFrom},
};

macro_rules! pow2size {
	{
		$($v:ident = $m:literal)*
	} => {
		/// Size represented as a power of 2.
		#[derive(Clone, Copy, Debug)]
		pub enum Pow2Size {
			$($v,)*
		}

		impl Pow2Size {
			fn approx_mask(mask: u32) -> Self {
				match mask.wrapping_add(1).trailing_zeros() {
					$(n if n == ($m as u32).trailing_ones() => Self::$v,)*
					_ => unreachable!(),
				}
			}

			fn into_mask(self) -> u32 {
				match self {
					$(Self::$v => $m,)*
				}
			}

			fn into_p2size(self) -> u8 {
				match self {
					$(Self::$v => ($m as u32).count_ones() as u8,)*
				}
			}
		}
	};
}

pow2size! {
	P0 = 0x0
	P1 = 0x1
	P2 = 0x3
	P3 = 0x7
	P4 = 0xf
	P5 = 0x1f
	P6 = 0x3f
	P7 = 0x7f
	P8 = 0xff
	P9 = 0x1ff
	P10 = 0x3ff
	P11 = 0x7ff
	P12 = 0xfff
	P13 = 0x1fff
	P14 = 0x3fff
	P15 = 0x7fff
	P16 = 0xffff
	P17 = 0x1_ffff
	P18 = 0x3_ffff
	P19 = 0x7_ffff
	P20 = 0xf_ffff
	P21 = 0x1f_ffff
	P22 = 0x3f_ffff
	P23 = 0x7f_ffff
	P24 = 0xff_ffff
	P25 = 0x1ff_ffff
	P26 = 0x3ff_ffff
	P27 = 0x7ff_ffff
	P28 = 0xfff_ffff
	P29 = 0x1fff_ffff
	P30 = 0x3fff_ffff
	P31 = 0x7fff_ffff
}

#[derive(Debug)]
pub struct Queue {
	/// The queue shared with the kernel.
	inner: io::Queue,
	/// How many requests are in flight. This is used to avoid submitting too many requests
	/// and potentially losing responses.
	requests_in_flight: u32,
}

impl Queue {
	pub fn new(requests_size: Pow2Size, responses_size: Pow2Size) -> error::Result<Self> {
		let base = syscall::create_io_queue(
			None,
			requests_size.into_p2size(),
			responses_size.into_p2size(),
		)?;
		Ok(Self {
			inner: io::Queue {
				base: base.cast(),
				requests_mask: requests_size.into_mask(),
				responses_mask: responses_size.into_mask(),
			},
			requests_in_flight: 0,
		})
	}

	pub fn requests_size(&self) -> Pow2Size {
		Pow2Size::approx_mask(self.inner.requests_mask)
	}

	pub fn responses_size(&self) -> Pow2Size {
		Pow2Size::approx_mask(self.inner.responses_mask)
	}

	pub fn submit(
		&mut self,
		user_data: u64,
		handle: Handle,
		request: Request,
	) -> Result<bool, Full> {
		// responses_mask + 1 = responses_len
		if self.inner.responses_mask < self.requests_in_flight {
			return Err(Full);
		}
		// SAFETY: requests_mask is not bogus.
		unsafe {
			let mut expect_response = true;
			self.inner
				.enqueue_request(match request {
					Request::Read { buffer } => io::Request::read_uninit(user_data, handle, buffer),
					Request::Write { buffer } => io::Request::write(user_data, handle, buffer),
					Request::Open { path } => io::Request::open(user_data, handle, path),
					Request::Create { path } => io::Request::create(user_data, handle, path),
					Request::Seek { from } => io::Request::seek(user_data, handle, from),
					Request::Poll => io::Request::poll(user_data, handle),
					Request::Close => {
						expect_response = false;
						io::Request::poll(user_data, handle)
					}
					Request::Peek { buffer } => io::Request::peek_uninit(user_data, handle, buffer),
					Request::Share { share } => io::Request::share(user_data, handle, share),
				})
				.map_err(|_| Full)?;
			if expect_response {
				self.requests_in_flight += 1;
			}
			Ok(expect_response)
		}
	}

	pub fn receive(&mut self) -> Option<Response> {
		// SAFETY: responses_mask is not bogus.
		let r = unsafe { self.inner.dequeue_response().ok() };
		if r.is_some() {
			self.requests_in_flight -= 1;
		}
		r
	}

	pub fn poll(&mut self) {
		syscall::process_io_queue(Some(self.inner.base.cast())).expect("failed to poll queue");
	}

	pub fn wait(&mut self) {
		syscall::wait_io_queue(Some(self.inner.base.cast())).expect("failed to wait queue");
	}
}

impl Drop for Queue {
	fn drop(&mut self) {
		while self.requests_in_flight > 0 {
			// TODO we should add a cancel request so we don't get potentially get stuck
			// if a response never arrives.
			self.poll();
			self.wait();
			while self.receive().is_some() {}
		}
		let _ = unsafe { syscall::destroy_io_queue(self.inner.base.cast()) };
	}
}

/// Any references have a static lifetime as it is the only way to safely guarantee a buffer
/// lives long enough for the kernel to write to it.
pub enum Request {
	Read {
		buffer: &'static mut [MaybeUninit<u8>],
	},
	Write {
		buffer: &'static [u8],
	},
	Open {
		path: &'static [u8],
	},
	Create {
		path: &'static [u8],
	},
	Seek {
		from: SeekFrom,
	},
	Poll,
	Close,
	Peek {
		buffer: &'static mut [MaybeUninit<u8>],
	},
	Share {
		share: Handle,
	},
}

#[derive(Debug)]
pub struct Full;
