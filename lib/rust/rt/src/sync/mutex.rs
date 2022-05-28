use super::RawMutex;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

#[derive(Debug, Default)]
pub struct Mutex<T> {
	lock: RawMutex,
	value: UnsafeCell<T>,
}

#[derive(Debug)]
pub struct MutexGuard<'a, T>(&'a Mutex<T>);

#[derive(Debug)]
pub struct Locked;

impl<T> Mutex<T> {
	pub const fn new(value: T) -> Self {
		Self {
			lock: RawMutex::new(),
			value: UnsafeCell::new(value),
		}
	}

	pub fn try_lock(&self) -> Result<MutexGuard<'_, T>, Locked> {
		self.lock.try_lock().then(|| MutexGuard(self)).ok_or(Locked)
	}

	pub fn lock(&self) -> MutexGuard<'_, T> {
		self.lock.lock();
		MutexGuard(self)
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
		self.0.lock.unlock();
	}
}
