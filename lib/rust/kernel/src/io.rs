//! # I/O
//!
//! All I/O is asynchronous and performed via shared ring buffers. A user process submits
//! requests to a request ring and receives a response in a response ring.
//!
//! To actually make the kernel parse requests, [`io_submit`] must be called.
//!
//! [`io_submit`]: crate::syscall::io_submit

// There is no generic RingBuffer type as that allows putting the user index as the first
// member in both the request and response ring, which saves a tiny bit of space in user
// programs on some platforms (e.g. 1 byte on x86 for LEA rd, [rs] vs LEA rd, [rs + off8])

use core::{
	mem::{self, MaybeUninit},
	ops::{Deref, DerefMut},
	ptr::NonNull,
	slice,
	sync::atomic::{AtomicU32, Ordering},
};

// We cast pointers to u64, so if pointers are 128 bits or larger things may break.
const _: usize = 8 - mem::size_of::<usize>();

pub type Handle = u32;

#[derive(Debug, PartialEq, Eq)]
pub struct Full;

#[derive(Debug, PartialEq, Eq)]
pub struct Empty;

/// A single request to submit to the kernel.
#[repr(C)]
pub struct Request {
	/// The type of request.
	pub ty: u8,
	/// Storage for 8-bit arguments.
	pub arguments_8: [u8; 3],
	/// Handle to object to perform the operation on.
	pub handle: Handle,
	/// Storage for 64-bit arguments. This storage is also used for pointers.
	pub arguments_64: [u64; 2],
	/// User data which will be returned with the response.
	pub user_data: u64,
}

impl Request {
	pub const READ: u8 = 0;
	pub const WRITE: u8 = 1;
	pub const GET_META: u8 = 2;
	pub const SET_META: u8 = 3;
	pub const OPEN: u8 = 4;
	pub const CREATE: u8 = 5;
	pub const DESTROY: u8 = 6;
	pub const SEEK: u8 = 7;
	pub const CLOSE: u8 = 8;
	pub const SHARE: u8 = 9;

	#[inline(always)]
	pub fn read(user_data: u64, handle: Handle, buf: &mut [u8]) -> Self {
		Self {
			ty: Self::READ,
			handle,
			arguments_64: [buf.as_ptr() as u64, buf.len() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn read_uninit(user_data: u64, handle: Handle, buf: &mut [MaybeUninit<u8>]) -> Self {
		Self {
			ty: Self::READ,
			handle,
			arguments_64: [buf.as_ptr() as u64, buf.len() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn write(user_data: u64, handle: Handle, buf: &[u8]) -> Self {
		Self {
			ty: Self::WRITE,
			handle,
			arguments_64: [buf.as_ptr() as u64, buf.len() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn get_meta(
		user_data: u64,
		handle: Handle,
		property: &TinySlice<u8>,
		value: &mut TinySlice<u8>,
	) -> Self {
		Self {
			ty: Self::GET_META,
			handle,
			arguments_8: [property.len_u8(), value.len_u8(), 0],
			arguments_64: [property.as_ptr() as u64, value.as_mut_ptr() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn get_meta_uninit(
		user_data: u64,
		handle: Handle,
		property: &TinySlice<u8>,
		value: &mut TinySlice<MaybeUninit<u8>>,
	) -> Self {
		Self {
			ty: Self::GET_META,
			handle,
			arguments_8: [property.len_u8(), value.len_u8(), 0],
			arguments_64: [property.as_ptr() as u64, value.as_mut_ptr() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn set_meta(
		user_data: u64,
		handle: Handle,
		property: &TinySlice<u8>,
		value: &TinySlice<u8>,
	) -> Self {
		Self {
			ty: Self::SET_META,
			handle,
			arguments_8: [property.len_u8(), value.len_u8(), 0],
			arguments_64: [property.as_ptr() as u64, value.as_ptr() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn open(user_data: u64, handle: Handle, path: &[u8]) -> Self {
		Self {
			ty: Self::OPEN,
			handle,
			arguments_64: [path.as_ptr() as u64, path.len() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn create(user_data: u64, handle: Handle, path: &[u8]) -> Self {
		Self {
			ty: Self::CREATE,
			handle,
			arguments_64: [path.as_ptr() as u64, path.len() as u64],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn seek(user_data: u64, handle: Handle, from: SeekFrom) -> Self {
		let (t, n) = from.into_raw();
		Self {
			ty: Self::SEEK,
			handle,
			arguments_8: [t, 0, 0],
			arguments_64: [n, 0],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn close(user_data: u64, handle: Handle) -> Self {
		Self { ty: Self::CLOSE, handle, user_data, ..Default::default() }
	}

	#[inline(always)]
	pub fn share(user_data: u64, handle: Handle, share: Handle) -> Self {
		Self {
			ty: Self::SHARE,
			handle,
			arguments_64: [share.into(), 0],
			user_data,
			..Default::default()
		}
	}

	#[inline(always)]
	pub fn destroy(user_data: u64, handle: Handle) -> Self {
		Self { ty: Self::DESTROY, handle, user_data, ..Default::default() }
	}
}

pub struct TinySlice<T>([T]);

impl<T> TinySlice<T> {
	#[inline]
	pub unsafe fn from_raw_parts<'a>(base: *const T, len: u8) -> &'a Self {
		unsafe { &*(slice::from_raw_parts(base, len.into()) as *const [T] as *const Self) }
	}

	#[inline]
	pub unsafe fn from_raw_parts_mut<'a>(base: *mut T, len: u8) -> &'a mut Self {
		unsafe { &mut *(slice::from_raw_parts_mut(base, len.into()) as *mut [T] as *mut Self) }
	}
}

impl<T> TryFrom<&[T]> for &TinySlice<T> {
	type Error = TooLarge;

	fn try_from(s: &[T]) -> Result<Self, Self::Error> {
		Ok(unsafe { &*(s.as_ref() as *const [T] as *const TinySlice<T>) })
	}
}

impl<T> TryFrom<&mut [T]> for &mut TinySlice<T> {
	type Error = TooLarge;

	fn try_from(s: &mut [T]) -> Result<Self, Self::Error> {
		Ok(unsafe { &mut *(s.as_mut() as *mut [T] as *mut TinySlice<T>) })
	}
}

// generic const exprs pls
macro_rules! arr_to_ts {
	($n:literal) => {
		impl<T> From<&[T; $n]> for &TinySlice<T> {
			fn from(s: &[T; $n]) -> Self {
				unsafe { &*(s.as_ref() as *const [T] as *const TinySlice<T>) }
			}
		}

		impl<T> From<&mut [T; $n]> for &mut TinySlice<T> {
			fn from(s: &mut [T; $n]) -> Self {
				unsafe { &mut *(s.as_mut() as *mut [T] as *mut TinySlice<T>) }
			}
		}
	};
	{ $($nn:literal)+ } => { $(arr_to_ts!($nn);)+ };
}
arr_to_ts! {
	0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
}

impl<T> TinySlice<T> {
	#[inline]
	pub fn len_u8(&self) -> u8 {
		self.0.len() as _
	}
}

impl<T> Deref for TinySlice<T> {
	type Target = [T];

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl<T> DerefMut for TinySlice<T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

#[derive(Debug)]
pub struct TooLarge;

pub struct DoIo<'a> {
	pub handle: Handle,
	pub op: DoIoOp<'a>,
}

pub enum DoIoOp<'a> {
	/// Read data from an object.
	Read { buf: &'a mut [u8] },
	/// Read data from an object.
	ReadUninit { buf: &'a mut [MaybeUninit<u8>] },
	/// Write data to an object.
	Write { data: &'a [u8] },
	/// Open an object at the given location.
	Open { path: &'a [u8] },
	/// Get meta-information about an object.
	GetMeta { property: &'a TinySlice<u8>, value: &'a mut TinySlice<u8> },
	/// Get meta-information about an object.
	GetMetaUninit { property: &'a TinySlice<u8>, value: &'a mut TinySlice<MaybeUninit<u8>> },
	/// Set meta-information about an object.
	SetMeta { property: &'a TinySlice<u8>, value: &'a TinySlice<u8> },
	/// Create an object at the given location.
	Create { path: &'a [u8] },
	/// Destroy an object at the given location.
	Destroy { path: &'a [u8] },
	/// Set the seek head.
	Seek { from: SeekFrom },
	/// Close a handle to an object.
	Close,
	/// Share an object.
	Share { share: Handle },
}

impl DoIo<'_> {
	#[inline]
	pub(crate) fn into_args(self) -> (u8, Handle, RawDoIo) {
		use RawDoIo::*;
		type R = Request;
		let h = self.handle;
		match self.op {
			DoIoOp::Read { buf } => (R::READ, h, N2(buf.as_ptr() as _, buf.len())),
			DoIoOp::ReadUninit { buf } => (R::READ, h, N2(buf.as_ptr() as _, buf.len())),
			DoIoOp::Write { data } => (R::WRITE, h, N2(data.as_ptr() as _, data.len())),
			DoIoOp::GetMeta { property, value } => (
				R::GET_META,
				h,
				N3(
					property.as_ptr() as _,
					value.as_mut_ptr() as _,
					property.len() | value.len() << 8,
				),
			),
			DoIoOp::GetMetaUninit { property, value } => (
				R::GET_META,
				h,
				N3(
					property.as_ptr() as _,
					value.as_mut_ptr() as _,
					property.len() | value.len() << 8,
				),
			),
			DoIoOp::SetMeta { property, value } => (
				R::SET_META,
				h,
				N3(
					property.as_ptr() as _,
					value.as_ptr() as _,
					property.len() | value.len() << 8,
				),
			),
			DoIoOp::Open { path } => (R::OPEN, h, N2(path.as_ptr() as _, path.len())),
			DoIoOp::Create { path } => (R::CREATE, h, N2(path.as_ptr() as _, path.len())),
			DoIoOp::Destroy { path } => (R::DESTROY, h, N2(path.as_ptr() as _, path.len())),
			DoIoOp::Seek { from } => {
				let (t, o) = from.into_raw();
				#[cfg(target_pointer_width = "64")]
				(R::SEEK, h, N2(t.into(), o as _))
			}
			DoIoOp::Close => return (R::CLOSE, h, N0),
			DoIoOp::Share { share } => (R::SHARE, h, N1(share as _)),
		}
	}
}

pub(crate) enum RawDoIo {
	N0,
	N1(usize),
	N2(usize, usize),
	N3(usize, usize, usize),
}

impl Default for Request {
	fn default() -> Self {
		Self {
			ty: u8::MAX,
			arguments_8: [0; 3],
			handle: Default::default(),
			arguments_64: [0; 2],
			user_data: 0,
		}
	}
}

/// A ring buffer of requests. The amount of entries is a power of two between 1 and 2^15
/// inclusive.
///
/// The index always *increments*.
#[repr(C)]
pub struct RequestRing {
	/// The index of the last entry the user process has queued.
	///
	/// Only the user process modifies this variable.
	pub user_index: AtomicU32,
	/// The index of the last entry the kernel has read. If this is equal to [`user_index`], the
	/// kernel has processed all submitted entries.
	///
	/// The user process *should not* modify this variable. The kernel will overwrite it with
	/// the proper value if it is modified anyways.
	pub kernel_index: AtomicU32,
	/// Entries. The actual size of the array is not 0 but variable dependent on the length
	/// negotiated beforehand.
	pub entries: [Request; 0],
}

impl RequestRing {
	/// Enqueue a request.
	///
	/// # Errors
	///
	/// This call will fail if the ring buffer is full.
	///
	/// # Safety
	///
	/// The passed mask *must* be accurate.
	#[inline]
	pub unsafe fn enqueue(&mut self, mask: u32, request: Request) -> Result<(), Full> {
		unsafe {
			enqueue(
				&self.kernel_index,
				&self.user_index,
				self.entries.as_mut_ptr(),
				mask,
				request,
			)
		}
	}

	/// Dequeue a request.
	///
	/// # Errors
	///
	/// This call will fail if the ring buffer is empty.
	///
	/// # Safety
	///
	/// The passed mask *must* be accurate.
	#[inline]
	pub unsafe fn dequeue(&mut self, mask: u32) -> Result<Request, Empty> {
		unsafe {
			dequeue(
				&self.kernel_index,
				&self.user_index,
				self.entries.as_mut_ptr(),
				mask,
			)
		}
	}

	/// Wait for the kernel to process all requests or until the closure returns `false`.
	pub fn wait_empty<F>(&self, mut f: F)
	where
		F: FnMut(u32) -> bool,
	{
		let w = self.user_index.load(Ordering::Relaxed);
		loop {
			let r = self.kernel_index.load(Ordering::Relaxed);
			if w == r || f(w.wrapping_sub(r)) {
				break;
			}
		}
	}
}

/// A single response from the kernel.
#[derive(Default)]
#[repr(C)]
pub struct Response {
	pub value: i64,
	/// User data that was associated with the request.
	pub user_data: u64,
}

/// A ring buffer of responses. The amount of entries is a power of two between 1 and 2^15
/// inclusive.
///
/// The index always *increments*.
#[repr(C)]
pub struct ResponseRing {
	/// The index of the last entry the user process has read.
	///
	/// Only the user process modifies this variable.
	pub user_index: AtomicU32,
	/// The index of the last entry the kernel has queued. If this is equal to [`user_index`], the
	/// kernel has processed all submitted entries.
	///
	/// The user process *should not* modify this variable. The kernel will overwrite it with
	/// the proper value if it is modified anyways.
	pub kernel_index: AtomicU32,
	/// Entries. The actual size of the array is not 0 but variable dependent on the length
	/// negotiated beforehand.
	pub entries: [Response; 0],
}

impl ResponseRing {
	/// Enqueue a request.
	///
	/// # Errors
	///
	/// This call will fail if the ring buffer is full.
	///
	/// # Safety
	///
	/// The passed mask *must* be accurate.
	#[inline]
	pub unsafe fn enqueue(&mut self, mask: u32, response: Response) -> Result<(), Full> {
		unsafe {
			enqueue(
				&self.user_index,
				&self.kernel_index,
				self.entries.as_mut_ptr(),
				mask,
				response,
			)
		}
	}

	/// Dequeue a request.
	///
	/// # Errors
	///
	/// This call will fail if the ring buffer is empty.
	///
	/// # Safety
	///
	/// The passed mask *must* be accurate.
	#[inline]
	pub unsafe fn dequeue(&mut self, mask: u32) -> Result<Response, Empty> {
		unsafe {
			dequeue(
				&self.user_index,
				&self.kernel_index,
				self.entries.as_mut_ptr(),
				mask,
			)
		}
	}

	/// Wait for the kernel to return a response or until the closure returns `false`.
	pub fn wait_any<F>(&self, mut f: F)
	where
		F: FnMut() -> bool,
	{
		let u = self.user_index.load(Ordering::Relaxed);
		loop {
			let k = self.kernel_index.load(Ordering::Relaxed);
			if u != k || f() {
				break;
			}
		}
	}
}

#[derive(Debug)]
pub struct Queue {
	pub base: NonNull<u8>,
	pub requests_mask: u32,
	pub responses_mask: u32,
}

impl Queue {
	#[inline]
	pub fn request_ring_size(mask: u32) -> usize {
		mem::size_of::<RequestRing>()
			+ usize::try_from(mask + 1).unwrap() * mem::size_of::<Request>()
	}

	#[inline]
	pub fn response_ring_size(mask: u32) -> usize {
		mem::size_of::<ResponseRing>()
			+ usize::try_from(mask + 1).unwrap() * mem::size_of::<Response>()
	}

	#[inline]
	pub fn total_size(req_mask: u32, resp_mask: u32) -> usize {
		Self::request_ring_size(req_mask) + Self::response_ring_size(resp_mask)
	}

	#[inline]
	pub unsafe fn request_ring(&self) -> &RequestRing {
		unsafe { self.base.cast::<RequestRing>().as_mut() }
	}

	#[inline]
	pub unsafe fn response_ring(&self) -> &ResponseRing {
		unsafe {
			&mut *self
				.base
				.cast::<u8>()
				.as_ptr()
				.add(Self::request_ring_size(self.requests_mask))
				.cast::<ResponseRing>()
		}
	}

	unsafe fn request_ring_mut(&mut self) -> &mut RequestRing {
		unsafe { self.base.cast::<RequestRing>().as_mut() }
	}

	unsafe fn response_ring_mut(&mut self) -> &mut ResponseRing {
		unsafe {
			&mut *self
				.base
				.cast::<u8>()
				.as_ptr()
				.add(Self::request_ring_size(self.requests_mask))
				.cast::<ResponseRing>()
		}
	}

	#[inline]
	pub unsafe fn enqueue_request(&mut self, request: Request) -> Result<(), Full> {
		let mask = self.requests_mask;
		unsafe { self.request_ring_mut().enqueue(mask, request) }
	}

	#[inline]
	pub unsafe fn dequeue_request(&mut self) -> Result<Request, Empty> {
		let mask = self.requests_mask;
		unsafe { self.request_ring_mut().dequeue(mask) }
	}

	#[inline]
	pub unsafe fn enqueue_response(&mut self, response: Response) -> Result<(), Full> {
		let mask = self.responses_mask;
		unsafe { self.response_ring_mut().enqueue(mask, response) }
	}

	#[inline]
	pub unsafe fn dequeue_response(&mut self) -> Result<Response, Empty> {
		let mask = self.responses_mask;
		unsafe { self.response_ring_mut().dequeue(mask) }
	}

	/// Wait for the kernel to process all requests or until the closure returns `false`.
	#[inline]
	pub fn wait_requests_empty<F>(&self, f: F)
	where
		F: FnMut(u32) -> bool,
	{
		unsafe { self.request_ring().wait_empty(f) }
	}

	/// Wait for the kernel to return a response or until the closure returns `false`.
	#[inline]
	pub fn wait_response_any<F>(&self, f: F)
	where
		F: FnMut() -> bool,
	{
		unsafe { self.response_ring().wait_any(f) }
	}

	/// Get how many responses are in the queue.
	#[inline]
	pub fn responses_available(&self) -> u32 {
		let ring = unsafe { self.response_ring() };
		ring.kernel_index
			.load(Ordering::Relaxed)
			.wrapping_sub(ring.user_index.load(Ordering::Relaxed))
	}
}

unsafe fn enqueue<E>(
	read: &AtomicU32,
	write: &AtomicU32,
	entries: *mut E,
	mask: u32,
	entry: E,
) -> Result<(), Full> {
	let r = read.load(Ordering::Relaxed);
	let w = write.load(Ordering::Relaxed);
	if r.wrapping_add(mask + 1) == w {
		return Err(Full);
	}
	// SAFETY: the mask forces the index to be in bounds.
	unsafe { entries.add((w & mask).try_into().unwrap()).write(entry) };
	write.store(w + 1, Ordering::Release);
	Ok(())
}

unsafe fn dequeue<E>(
	read: &AtomicU32,
	write: &AtomicU32,
	entries: *mut E,
	mask: u32,
) -> Result<E, Empty> {
	let r = read.load(Ordering::Relaxed);
	let w = write.load(Ordering::Relaxed);
	if r == w {
		return Err(Empty);
	}
	// SAFETY: the mask forces the index to be in bounds.
	let e = unsafe { entries.add((r & mask).try_into().unwrap()).read() };
	read.store(r + 1, Ordering::Release);
	Ok(e)
}

#[derive(Clone, Copy, Debug)]
pub enum SeekFrom {
	Start(u64),
	End(i64),
	Current(i64),
}

impl SeekFrom {
	/// Modify an offset while accounting for overflow.
	pub fn apply(self, offset: usize, max: usize) -> usize {
		match self {
			Self::Start(n) => n.try_into().unwrap_or(max),
			Self::End(n) => max.saturating_sub((-n).try_into().unwrap_or(usize::MAX)),
			Self::Current(n) => {
				if n >= 0 {
					offset
						.saturating_add(n.try_into().unwrap_or(usize::MAX))
						.min(max)
				} else {
					offset.saturating_sub((-n).try_into().unwrap_or(usize::MAX))
				}
			}
		}
	}

	pub fn into_raw(self) -> (u8, u64) {
		match self {
			Self::Start(n) => (0, n),
			Self::End(n) => (1, n as u64),
			Self::Current(n) => (2, n as u64),
		}
	}

	pub fn try_from_raw(t: u8, n: u64) -> Result<Self, ()> {
		match t {
			0 => Ok(Self::Start(n)),
			1 => Ok(Self::End(n as i64)),
			2 => Ok(Self::Current(n as i64)),
			_ => Err(()),
		}
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn enqueue_request() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_request(Request::default()).unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
		}
	}

	#[test]
	fn dequeue_request() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue
				.enqueue_request(Request { user_data: 1337, ..Default::default() })
				.unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			let req = queue.dequeue_request().unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 1);
			assert_eq!(req.user_data, 1337);
		}
	}

	#[test]
	fn enqueue_2_requests() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_request(Request::default()).unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			queue.enqueue_request(Request::default()).unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 2);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
		}
	}

	#[test]
	fn enqueue_8_requests() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 7, responses_mask: 7 };
		unsafe {
			for i in 1..9 {
				queue.enqueue_request(Request::default()).unwrap();
				assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), i);
				assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			}
		}
	}

	#[test]
	fn dequeue_8_requests() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 7, responses_mask: 7 };
		unsafe {
			for i in 1..9 {
				queue
					.enqueue_request(Request { user_data: i.into(), ..Default::default() })
					.unwrap();
				assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), i);
				assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			}
			for i in 1..9 {
				let req = queue.dequeue_request().unwrap();
				assert_eq!(req.user_data, i.into());
				assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 8);
				assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), i);
			}
		}
	}

	#[test]
	fn fail_enqueue_request() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_request(Request::default()).unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			queue.enqueue_request(Request::default()).unwrap();
			assert_eq!(queue.request_ring().user_index.load(Ordering::Relaxed), 2);
			assert_eq!(queue.request_ring().kernel_index.load(Ordering::Relaxed), 0);
			assert_eq!(Err(Full), queue.enqueue_request(Request::default()));
		}
	}

	#[test]
	fn fail_dequeue_request() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			assert!(queue.dequeue_request().is_err());
		}
	}

	#[test]
	fn enqueue_response() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_response(Response::default()).unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				1
			);
		}
	}

	#[test]
	fn dequeue_response() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue
				.enqueue_response(Response { user_data: 1337, ..Default::default() })
				.unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				1
			);
			let req = queue.dequeue_response().unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 1);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				1
			);
			assert_eq!(req.user_data, 1337);
		}
	}

	#[test]
	fn enqueue_2_responses() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_response(Response::default()).unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				1
			);
			queue.enqueue_response(Response::default()).unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				2
			);
		}
	}

	#[test]
	fn enqueue_8_responses() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 7, responses_mask: 7 };
		unsafe {
			for i in 1..9 {
				queue.enqueue_response(Response::default()).unwrap();
				assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
				assert_eq!(
					queue.response_ring().kernel_index.load(Ordering::Relaxed),
					i
				);
			}
		}
	}

	#[test]
	fn dequeue_8_responses() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 7, responses_mask: 7 };
		unsafe {
			for i in 1..9 {
				queue
					.enqueue_response(Response { user_data: i.into(), ..Default::default() })
					.unwrap();
				assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
				assert_eq!(
					queue.response_ring().kernel_index.load(Ordering::Relaxed),
					i
				);
			}
			for i in 1..9 {
				let req = queue.dequeue_response().unwrap();
				assert_eq!(req.user_data, i.into());
				assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), i);
				assert_eq!(
					queue.response_ring().kernel_index.load(Ordering::Relaxed),
					8
				);
			}
		}
	}

	#[test]
	fn fail_enqueue_response() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			queue.enqueue_response(Response::default()).unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				1
			);
			queue.enqueue_response(Response::default()).unwrap();
			assert_eq!(queue.response_ring().user_index.load(Ordering::Relaxed), 0);
			assert_eq!(
				queue.response_ring().kernel_index.load(Ordering::Relaxed),
				2
			);
			assert_eq!(Err(Full), queue.enqueue_response(Response::default()));
		}
	}

	#[test]
	fn fail_dequeue_response() {
		let base = Box::new([0; 4096]);
		let mut queue =
			Queue { base: NonNull::from(&*base).cast(), requests_mask: 1, responses_mask: 1 };
		unsafe {
			assert!(queue.dequeue_response().is_err());
		}
	}
}
