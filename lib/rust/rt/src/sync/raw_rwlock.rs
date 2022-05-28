use crate::thread;
use core::{
	intrinsics,
	sync::atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
pub struct RawRwLock {
	lock: AtomicUsize,
}

const WRITE_LOCK_BIT: usize = 1 << (usize::BITS - 1);

impl RawRwLock {
	pub const fn new() -> Self {
		Self {
			lock: AtomicUsize::new(0),
		}
	}

	/// Wait for the write bit to be cleared.
	#[inline]
	fn wait_for_write_bit_clear(&self, v: &mut usize) {
		// Wait for the write lock bit to be cleared
		while *v & WRITE_LOCK_BIT != 0 {
			thread::yield_now();
			*v = self.lock.load(Ordering::Relaxed);
		}
		debug_assert_eq!(*v & WRITE_LOCK_BIT, 0, "lock bit is already set");
	}

	#[inline]
	pub fn try_read(&self) -> bool {
		let v = self.lock.load(Ordering::Relaxed);
		if v & WRITE_LOCK_BIT == 0 {
			false
		} else {
			check_read_overflow(v + 1);
			self.lock
				.compare_exchange(v, v + 1, Ordering::Acquire, Ordering::Relaxed)
				.is_ok()
		}
	}

	#[inline]
	pub fn read(&self) {
		let mut v = self.lock.load(Ordering::Relaxed);
		loop {
			self.wait_for_write_bit_clear(&mut v);
			check_read_overflow(v + 1);
			match self
				.lock
				.compare_exchange(v, v + 1, Ordering::Acquire, Ordering::Relaxed)
			{
				Ok(_) => break,
				Err(nv) => v = nv,
			}
		}
	}

	#[inline]
	pub fn try_write(&self) -> bool {
		self.lock
			.compare_exchange(0, usize::MAX, Ordering::Acquire, Ordering::Relaxed)
			.is_ok()
	}

	#[inline]
	pub fn write(&self) {
		let mut v = self.lock.load(Ordering::Relaxed);
		// Indicate we're trying to acquire a write lock.
		loop {
			self.wait_for_write_bit_clear(&mut v);
			// Try to set the lock bit, blocking readers & other writers.
			match self.lock.compare_exchange_weak(
				v,
				v | WRITE_LOCK_BIT,
				Ordering::Acquire,
				Ordering::Relaxed,
			) {
				Ok(_) => break,
				Err(nv) => v = nv,
			}
		}
		// Now that the write bit is set we just need to wait for all readers to bugger off.
		while self
			.lock
			.compare_exchange(
				WRITE_LOCK_BIT,
				usize::MAX,
				Ordering::Acquire,
				Ordering::Relaxed,
			)
			.is_err()
		{
			thread::yield_now();
		}
	}

	#[inline]
	pub fn read_unlock(&self) {
		// No writes can have occured, so Relaxed ordering is fine.
		self.lock.fetch_sub(1, Ordering::Relaxed);
	}

	#[inline]
	pub fn write_unlock(&self) {
		debug_assert_eq!(
			self.lock.load(Ordering::Relaxed),
			usize::MAX,
			"write lock got released"
		);
		self.lock.store(0, Ordering::Release);
	}
}

impl Default for RawRwLock {
	fn default() -> Self {
		Self::new()
	}
}

/// Ensure the counter didn't overflow. If it did, abort immediately.
fn check_read_overflow(v: usize) {
	if v & WRITE_LOCK_BIT == 0 {
		intrinsics::abort()
	}
}
