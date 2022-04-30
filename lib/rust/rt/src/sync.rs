use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug)]
pub struct Mutex<T> {
	// We use an u32 because some platforms such as RISC-V don't have native
	// u8 or u16 atomic instructions. While it can be emulated it is quite a bit less efficient.
	lock: AtomicU32,
	value: UnsafeCell<T>,
}

#[derive(Debug)]
pub struct MutexGuard<'a, T>(&'a Mutex<T>);

#[derive(Debug)]
pub struct Locked;

const UNLOCKED: u32 = 0;
const LOCKED: u32 = 1;

impl<T> Mutex<T> {
	pub const fn new(value: T) -> Self {
		Self {
			lock: AtomicU32::new(UNLOCKED),
			value: UnsafeCell::new(value),
		}
	}

	pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, Locked> {
		self.lock
			.compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
			.map(|_| MutexGuard(self))
			.map_err(|_| Locked)
	}

	pub fn lock(&self) -> MutexGuard<'_, T> {
		loop {
			match self.try_lock() {
				Ok(guard) => break guard,
				Err(Locked) => {}
			}
		}
	}
}

// SAFETY: we synchronize access to the inner value.
unsafe impl<T> Sync for Mutex<T> {}

impl<T> Deref for MutexGuard<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		// SAFETY: we own the lock, so there can be no other mutable references to the value.
		unsafe { &*self.0.value.get() }
	}
}

impl<T> DerefMut for MutexGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		// SAFETY: we own the lock, so there can be no other mutable references to the value.
		unsafe { &mut *self.0.value.get() }
	}
}

impl<T> Drop for MutexGuard<'_, T> {
	fn drop(&mut self) {
		self.0.lock.store(UNLOCKED, Ordering::Release);
	}
}
