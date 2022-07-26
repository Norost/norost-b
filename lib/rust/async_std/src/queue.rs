use crate::io::{Buf, BufMut};
use alloc::boxed::Box;
use core::{sync::atomic::Ordering, time::Duration};
use io_queue_rt::{Full, Pow2Size, Queue};

static IO_QUEUE_KEY: rt::tls::AtomicKey = rt::tls::AtomicKey::default();

/// Try to submit a request, blocking & retrying if the queue is full.
pub fn submit<F, B, R>(f: F, mut buf: B) -> R
where
	F: Fn(&'static Queue, B) -> Result<R, Full<B>>,
	B: Buf,
{
	submit2(|q, b, _| f(q, b).map_err(|Full(b)| Full((b, ()))), buf, ())
}

/// Try to submit a request, blocking & retrying if the queue is full.
pub fn submit_mut<F, B, R>(f: F, mut buf: B) -> R
where
	F: Fn(&'static Queue, B) -> Result<R, Full<B>>,
	B: BufMut,
{
	submit2(|q, _, b| f(q, b).map_err(|Full(b)| Full(((), b))), (), buf)
}

/// Try to submit a request, blocking & retrying if the queue is full.
pub fn submit2<F, B, Bm, R>(f: F, mut buf: B, mut buf2: Bm) -> R
where
	F: Fn(&'static Queue, B, Bm) -> Result<R, Full<(B, Bm)>>,
	B: Buf,
	Bm: BufMut,
{
	let q = get();
	loop {
		(buf, buf2) = match f(q, buf, buf2) {
			Ok(r) => return r,
			Err(Full(b)) => b,
		};
		q.poll();
		q.wait(Duration::MAX);
		q.process();
	}
}

pub fn poll() -> rt::time::Monotonic {
	let q = get();
	let t = q.poll();
	q.process();
	t
}

pub fn wait(timeout: Duration) -> rt::time::Monotonic {
	let q = get();
	let t = q.poll();
	let t = q.wait(timeout).unwrap_or(t);
	q.process();
	t
}

pub fn get() -> &'static Queue {
	// Get or allocate key
	let mut key = IO_QUEUE_KEY.load(Ordering::Relaxed);
	if key == rt::tls::Key::default() {
		let k = rt::tls::allocate(Some(destroy_queue))
			.expect("failed to allocate TLS storage for I/O queue");
		match IO_QUEUE_KEY.compare_exchange(key, k, Ordering::Relaxed, Ordering::Relaxed) {
			Ok(_) => key = k,
			Err(nk) => {
				// SAFETY: we're not using the allocated key
				unsafe { rt::tls::free(k) };
				key = nk;
			}
		};
	}

	// Get or create queue
	// SAFETY: we have a valid key.
	// FIXME it's not practical to check if TLS is initialized without marking everything as
	// unsafe.
	let mut queue = unsafe { rt::tls::get(key) }.cast::<Queue>();
	if queue.is_null() {
		// 2^6 * (32 + 16) = 3072 < 4096, i.e. it fits in one page.
		let q = Queue::new(Pow2Size::P6, Pow2Size::P6).expect("failed to create I/O queue");
		queue = Box::into_raw(Box::new(q));
		// SAFETY: we have a valid key.
		unsafe { rt::tls::set(key, queue.cast()) };
	}

	// SAFETY: queue is not Sync, so references to it are not Send.
	// The queue is only destroyed when the thread itself is destroyed, so
	// it cannot be used afterwards by this thread nor other threads.
	unsafe { &*(queue as *const _) }
}

/// # Safety
///
/// `get` may not be called after this in the same thread.
unsafe extern "C" fn destroy_queue(queue: *mut ()) {
	let queue = unsafe { Box::from_raw(queue.cast::<Queue>()) };
	// FIXME we need a way to ensure all requests have been submitted.
	queue.poll();
}
