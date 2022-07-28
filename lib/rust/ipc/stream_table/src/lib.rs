#![no_std]
#![feature(core_intrinsics)]
#![feature(maybe_uninit_slice, maybe_uninit_uninit_array)]
#![feature(int_roundings)]
#![cfg_attr(not(debug_assertions), deny(unused))]

mod raw {
	norost_ipc_spec::compile!(core::include_str!("../../../../ipc/stream_table.ipc"));
}
mod buffer;

pub mod stack;

pub use buffer::*;
pub use raw::Id as JobId;

type Handle = u32;

use core::{
	num::Wrapping,
	ptr::{self, NonNull},
	sync::atomic::{AtomicU32, Ordering},
};

const REQUESTS_MASK: u32 = (1 << 7) - 1;
const RESPONSES_MASK: u32 = (1 << 7) - 1;

struct Queue {
	base: NonNull<u8>,
}

pub struct ClientQueue {
	base: Queue,
	request_tail: Wrapping<u32>,
	response_head: Wrapping<u32>,
}

pub struct ServerQueue {
	base: Queue,
	response_tail: Wrapping<u32>,
	request_head: Wrapping<u32>,
}

macro_rules! index_ref {
	($f:ident $o:ident) => {
		fn $f(&self) -> &AtomicU32 {
			const OFFSET: usize = {
				assert!(raw::Queue::$o().bits() % 32 == 0);
				raw::Queue::$o().bytes_bits().0
			};
			unsafe { &*self.base.as_ptr().add(OFFSET).cast() }
		}
	};
}

impl Queue {
	index_ref!(request_tail_ref request_tail_offset);
	index_ref!(request_head_ref request_head_offset);
	index_ref!(response_tail_ref response_tail_offset);
	index_ref!(response_head_ref response_head_offset);
	index_ref!(buffer_head_ref buffer_head_offset);

	fn response_ptr(&self, index: u32) -> *mut raw::Response {
		let i = (index & RESPONSES_MASK) as usize;
		let (offset, bits) = raw::Queue::responses_offset_at(i).bytes_bits();
		assert_eq!(bits, 0);
		unsafe { self.base.as_ptr().add(offset).cast() }
	}

	fn request_ptr(&self, index: u32) -> *mut raw::Request {
		let i = (index & REQUESTS_MASK) as usize;
		let (offset, bits) = raw::Queue::requests_offset_at(i).bytes_bits();
		assert_eq!(bits, 0);
		unsafe { self.base.as_ptr().add(offset).cast() }
	}
}

impl ServerQueue {
	/// # Safety
	///
	/// `base` must point to a valid memory region.
	#[inline(always)]
	pub unsafe fn new(base: NonNull<u8>) -> Self {
		Self {
			base: Queue { base },
			response_tail: Wrapping(0),
			request_head: Wrapping(0),
		}
	}

	#[inline(always)]
	pub fn into_raw(self) -> NonNull<u8> {
		self.base.base
	}

	/// # Note
	///
	/// This does not check if the queue is full.
	#[inline]
	fn enqueue(&mut self, job_id: JobId, response: Response) {
		let mut v = raw::ResponseValue::default();
		match response {
			Response::Error(e) => v.set_error(e),
			Response::Position(p) => v.set_position(p),
			Response::Handle(h) => v.set_handle(h),
			Response::Amount(a) => v.set_amount(a),
			Response::Raw(a) => v.set_raw(a),
			Response::Slice(s) => v.set_slice(s.into_raw()),
		};
		let mut r = raw::Response::default();
		r.set_id(job_id);
		r.set_value(v);
		unsafe { ptr::write_volatile(self.base.response_ptr(self.response_tail.0), r) }
		self.response_tail += 1;
		self.base
			.response_tail_ref()
			.store(self.response_tail.0, Ordering::Release);
	}

	#[inline]
	pub fn try_enqueue(&mut self, job_id: JobId, response: Response) -> Result<(), Full> {
		let server_head = self.base.response_head_ref().load(Ordering::Relaxed);
		(server_head != (self.response_tail - Wrapping(RESPONSES_MASK + 1)).0)
			.then(|| self.enqueue(job_id, response))
			.ok_or(Full)
	}

	#[inline]
	pub fn dequeue(&mut self) -> Option<(Handle, Request)> {
		let index = self.base.request_tail_ref().load(Ordering::Acquire);
		(index != self.request_head.0).then(|| {
			let r = unsafe { ptr::read_volatile(self.base.request_ptr(self.request_head.0)) };
			self.request_head += 1;
			self.base
				.request_head_ref()
				.store(self.request_head.0, Ordering::Relaxed);
			let (job_id, handle) = (r.id(), r.handle());
			let args = r.args();
			type T = raw::RequestType;
			type R = Request;
			type S = SeekFrom;
			(
				handle,
				match r.ty().unwrap() {
					T::Read => R::Read {
						job_id,
						amount: args.amount(),
					},
					T::Write => R::Write {
						job_id,
						data: Slice::from_raw(args.slice()),
					},
					T::Open => R::Open {
						job_id,
						path: Slice::from_raw(args.slice()),
					},
					T::GetMeta => R::GetMeta {
						job_id,
						property: Slice::from_raw(args.slice()),
					},
					T::SetMeta => R::SetMeta {
						job_id,
						property_value: Slice::from_raw(args.slice()),
					},
					T::Close => R::Close,
					T::Create => R::Create {
						job_id,
						path: Slice::from_raw(args.slice()),
					},
					T::Destroy => R::Destroy {
						job_id,
						path: Slice::from_raw(args.slice()),
					},
					T::SeekStart => R::Seek {
						job_id,
						from: S::Start(args.offset_u()),
					},
					T::SeekCurrent => R::Seek {
						job_id,
						from: S::Current(args.offset_s()),
					},
					T::SeekEnd => R::Seek {
						job_id,
						from: S::End(args.offset_s() as _),
					},
					T::Share => R::Share {
						job_id,
						share: args.share(),
					},
				},
			)
		})
	}

	#[inline]
	pub fn buffer_head_ref(&self) -> &AtomicU32 {
		self.base.buffer_head_ref()
	}
}

impl ClientQueue {
	/// # Safety
	///
	/// `base` must point to a valid memory region.
	#[inline(always)]
	pub unsafe fn new(base: NonNull<u8>) -> Self {
		Self {
			base: Queue { base },
			response_head: Wrapping(0),
			request_tail: Wrapping(0),
		}
	}

	#[inline(always)]
	pub fn into_raw(self) -> NonNull<u8> {
		self.base.base
	}

	/// # Note
	///
	/// This does not check if the queue is full.
	#[inline]
	fn enqueue(&mut self, handle: Handle, request: Request) {
		let mut v = raw::RequestArgs::default();
		type T = raw::RequestType;
		type R = Request;
		let (job_id, ty, ()) = match request {
			R::Read { job_id, amount } => (job_id, T::Read, v.set_amount(amount)),
			R::Write { job_id, data } => (job_id, T::Write, v.set_slice(data.into_raw())),
			R::GetMeta { job_id, property } => {
				(job_id, T::GetMeta, v.set_slice(property.into_raw()))
			}
			R::SetMeta {
				job_id,
				property_value,
			} => (job_id, T::SetMeta, v.set_slice(property_value.into_raw())),
			R::Open { job_id, path } => (job_id, T::Open, v.set_slice(path.into_raw())),
			R::Close => (JobId::default(), T::Close, ()),
			R::Create { job_id, path } => (job_id, T::Create, v.set_slice(path.into_raw())),
			R::Destroy { job_id, path } => (job_id, T::Destroy, v.set_slice(path.into_raw())),
			R::Seek { job_id, from } => match from {
				SeekFrom::Start(f) => (job_id, T::SeekStart, v.set_offset_u(f)),
				SeekFrom::Current(f) => (job_id, T::SeekCurrent, v.set_offset_s(f)),
				SeekFrom::End(f) => (job_id, T::SeekEnd, v.set_offset_s(f)),
			},
			R::Share { job_id, share } => (job_id, T::Share, v.set_share(share)),
		};
		let mut r = raw::Request::default();
		r.set_ty(ty);
		r.set_id(job_id);
		r.set_handle(handle);
		r.set_args(v);
		unsafe { ptr::write_volatile(self.base.request_ptr(self.request_tail.0), r) }
		self.request_tail += 1;
		self.base
			.request_tail_ref()
			.store(self.request_tail.0, Ordering::Release);
	}

	#[inline]
	pub fn try_enqueue(&mut self, handle: Handle, request: Request) -> Result<(), Full> {
		let server_head = self.base.request_head_ref().load(Ordering::Relaxed);
		(server_head != (self.request_tail - Wrapping(REQUESTS_MASK + 1)).0)
			.then(|| self.enqueue(handle, request))
			.ok_or(Full)
	}

	#[inline]
	pub fn requests_enqueued(&self) -> u32 {
		(self.request_tail - Wrapping(self.base.request_head_ref().load(Ordering::Relaxed))).0
	}

	#[inline]
	pub fn dequeue(&mut self) -> Option<(JobId, AnyResponse)> {
		let index = self.base.response_tail_ref().load(Ordering::Acquire);
		(index != self.response_head.0).then(|| {
			let r = unsafe { ptr::read_volatile(self.base.response_ptr(self.response_head.0)) };
			self.response_head += 1;
			self.base
				.response_head_ref()
				.store(self.response_head.0, Ordering::Relaxed);
			(r.id(), AnyResponse(r.value().raw()))
		})
	}

	#[inline]
	pub fn buffer_head_ref(&self) -> &AtomicU32 {
		self.base.buffer_head_ref()
	}
}

pub enum SeekFrom {
	Start(u64),
	Current(i64),
	End(i64),
}

pub enum Request {
	Read {
		job_id: JobId,
		amount: u32,
	},
	Write {
		job_id: JobId,
		data: Slice,
	},
	GetMeta {
		job_id: JobId,
		property: Slice,
	},
	SetMeta {
		job_id: JobId,
		property_value: Slice,
	},
	Open {
		job_id: JobId,
		path: Slice,
	},
	Close,
	Create {
		job_id: JobId,
		path: Slice,
	},
	Destroy {
		job_id: JobId,
		path: Slice,
	},
	Seek {
		job_id: JobId,
		from: SeekFrom,
	},
	Share {
		job_id: JobId,
		share: Handle,
	},
}

pub enum Response {
	Error(i64),
	Position(u64),
	Handle(Handle),
	Amount(u32),
	Raw(u64),
	Slice(Slice),
}

pub struct AnyResponse(u64);

impl AnyResponse {
	pub fn get(&self) -> Result<u64, i16> {
		(self.0 < u64::MAX & !4095)
			.then(|| self.0)
			.ok_or(self.0 as _)
	}

	pub fn as_slice(&self) -> Result<Slice, i16> {
		use norost_ipc_spec::Data;
		self.get()
			.map(|v| Slice::from_raw(raw::Slice::from_raw(&v.to_le_bytes(), 0)))
	}
}

#[derive(Clone, Copy, Debug)]
pub struct Slice {
	pub offset: u32,
	pub length: u32,
}

impl Slice {
	fn from_raw(raw: raw::Slice) -> Self {
		Self {
			offset: raw.offset(),
			length: raw.length(),
		}
	}

	fn into_raw(self) -> raw::Slice {
		let mut s = raw::Slice::default();
		s.set_offset(self.offset);
		s.set_length(self.length);
		s
	}
}

#[derive(Debug)]
pub struct Full;
