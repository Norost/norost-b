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

use super::syscall::TableId;
use core::mem::{self, MaybeUninit};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};

pub type Handle = u32;

#[derive(Debug)]
pub struct Full;

#[derive(Debug)]
pub struct Empty;

/// A single request to submit to the kernel.
#[repr(C)]
pub struct Request {
	/// The type of request.
	pub ty: u8,
	/// Storage for 8-bit arguments.
	pub arguments_8: [u8; 3],
	/// Storage for 32-bit arguments.
	pub arguments_32: [u32; 2],
	/// Storage for 32- or 64-bit arguments, depending on architecture.
	pub arguments_ptr: [usize; 2],
	/// User data which will be returned with the response.
	pub user_data: usize,
	/// Storage for 64-bit arguments.
	pub arguments_64: [u64; 1],
}

impl Request {
	// NB for later: reuse READ for take_table_job and WRITE for finish_table_job.
	// It'll be cleaner & simpler.
	pub const READ: u8 = 0;
	pub const WRITE: u8 = 1;
	pub const OPEN: u8 = 2;
	pub const CREATE: u8 = 3;
	pub const QUERY: u8 = 4;
	pub const QUERY_NEXT: u8 = 5;
	pub const TAKE_JOB: u8 = 6;
	pub const FINISH_JOB: u8 = 7;
	pub const SEEK: u8 = 8;
	pub const POLL: u8 = 9;
	pub const CLOSE: u8 = 10;

	pub fn read(user_data: usize, handle: Handle, buf: &mut [u8]) -> Self {
		Self {
			ty: Self::READ,
			arguments_32: [handle, 0],
			arguments_ptr: [buf.as_ptr() as usize, buf.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn read_uninit(user_data: usize, handle: Handle, buf: &mut [MaybeUninit<u8>]) -> Self {
		Self {
			ty: Self::READ,
			arguments_32: [handle, 0],
			arguments_ptr: [buf.as_ptr() as usize, buf.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn write(user_data: usize, handle: Handle, buf: &[u8]) -> Self {
		Self {
			ty: Self::WRITE,
			// FIXME make handles 32-bit
			arguments_32: [handle.try_into().unwrap(), 0],
			arguments_ptr: [buf.as_ptr() as usize, buf.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn open(user_data: usize, table: TableId, path: &[u8]) -> Self {
		Self {
			ty: Self::OPEN,
			arguments_32: [table, 0],
			arguments_ptr: [path.as_ptr() as usize, path.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn create(user_data: usize, table: TableId, path: &[u8]) -> Self {
		Self {
			ty: Self::CREATE,
			arguments_32: [table, 0],
			arguments_ptr: [path.as_ptr() as usize, path.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn query(user_data: usize, table: TableId, path: &[u8]) -> Self {
		Self {
			ty: Self::QUERY,
			arguments_32: [table, 0],
			arguments_ptr: [path.as_ptr() as usize, path.len()],
			user_data,
			..Default::default()
		}
	}

	pub fn query_next(user_data: usize, handle: Handle, info: &mut ObjectInfo) -> Self {
		Self {
			ty: Self::QUERY_NEXT,
			arguments_32: [handle, 0],
			arguments_ptr: [info as *const _ as usize, 0],
			user_data,
			..Default::default()
		}
	}

	pub fn take_job(user_data: usize, table: Handle, job: &mut Job) -> Self {
		Self {
			ty: Self::TAKE_JOB,
			arguments_32: [table, 0],
			arguments_ptr: [job as *mut _ as usize, 0],
			user_data,
			..Default::default()
		}
	}

	pub fn finish_job(user_data: usize, table: Handle, job: &Job) -> Self {
		Self {
			ty: Self::FINISH_JOB,
			arguments_32: [table, 0],
			arguments_ptr: [job as *const _ as usize, 0],
			user_data,
			..Default::default()
		}
	}

	pub fn seek(user_data: usize, handle: Handle, from: SeekFrom, offset: &mut u64) -> Self {
		let (t, n) = from.into_raw();
		Self {
			ty: Self::SEEK,
			arguments_8: [t, 0, 0],
			arguments_32: [handle, 0],
			arguments_64: [n],
			arguments_ptr: [offset as *mut _ as usize, 0],
			user_data,
			..Default::default()
		}
	}

	pub fn poll(user_data: usize, handle: Handle) -> Self {
		Self {
			ty: Self::POLL,
			arguments_32: [handle, 0],
			user_data,
			..Default::default()
		}
	}

	pub fn close(user_data: usize, handle: Handle) -> Self {
		Self {
			ty: Self::CLOSE,
			arguments_32: [handle, 0],
			user_data,
			..Default::default()
		}
	}
}

impl Default for Request {
	fn default() -> Self {
		Self {
			ty: u8::MAX,
			arguments_8: [0; 3],
			arguments_32: [0; 2],
			arguments_64: [0; 1],
			arguments_ptr: [0; 2],
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
#[repr(C)]
pub struct Response {
	pub value: isize,
	/// User data that was associated with the request.
	pub user_data: usize,
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

	#[inline]
	pub unsafe fn request_ring_mut(&mut self) -> &mut RequestRing {
		unsafe { self.base.cast::<RequestRing>().as_mut() }
	}

	#[inline]
	pub unsafe fn response_ring_mut(&mut self) -> &mut ResponseRing {
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
		ring.user_index
			.load(Ordering::Relaxed)
			.wrapping_sub(ring.kernel_index.load(Ordering::Relaxed))
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

#[derive(Debug, Default)]
#[repr(C)]
pub struct Job {
	pub ty: u8,
	pub flags: [u8; 3],
	pub job_id: JobId,
	pub buffer_size: u32,
	pub operation_size: u32,
	pub handle: Handle,
	pub buffer: Option<NonNull<u8>>,
	pub from_anchor: u8,
	pub from_offset: u64,
}

impl Job {
	pub const OPEN: u8 = 0;
	pub const READ: u8 = 1;
	pub const WRITE: u8 = 2;
	pub const QUERY: u8 = 3;
	pub const CREATE: u8 = 4;
	pub const QUERY_NEXT: u8 = 5;
	pub const SEEK: u8 = 6;
	pub const CLOSE: u8 = 7;
}

pub type JobId = u32;

#[derive(Debug)]
#[repr(C)]
pub struct ObjectInfo {
	// FIXME potentially UB if modified
	pub path_ptr: *mut u8,
	pub path_len: usize,
	pub path_capacity: usize,
}

impl ObjectInfo {
	pub fn new<'a>(path_buffer: &'a mut [u8]) -> Self {
		Self {
			path_ptr: path_buffer.as_mut_ptr(),
			path_capacity: path_buffer.len(),
			..Default::default()
		}
	}
}

impl Default for ObjectInfo {
	fn default() -> Self {
		Self {
			path_ptr: core::ptr::null_mut(),
			path_len: Default::default(),
			path_capacity: Default::default(),
		}
	}
}
