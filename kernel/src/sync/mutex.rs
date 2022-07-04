use crate::{arch::sync, scheduler::Thread};
use core::{
	cell::UnsafeCell,
	ops::{Deref, DerefMut},
};

/// A very basic spinlock implementation. Intended for short sections that are mostly uncontended.
pub struct Mutex<T> {
	lock: sync::Mutex,
	value: UnsafeCell<T>,
}

impl<T> Mutex<T> {
	pub const fn new(value: T) -> Self {
		Self {
			lock: Default::default(),
			value: UnsafeCell::new(value),
		}
	}

	#[cfg_attr(debug_assertions, track_caller)]
	#[inline]
	pub fn lock(&self) -> Guard<T> {
		// Mutexes may never be locked inside an ISR since it can lead to deadlocks.
		debug_assert!(
			crate::arch::interrupts_enabled(),
			"interrupts are disabled. Is the mutex being locked inside an ISR?"
		);
		while !self.lock.try_lock() {
			Thread::yield_current()
		}
		Guard { lock: self }
	}

	/// Borrow the lock mutably, which is safe since mutable references are always unique.
	#[allow(dead_code)]
	#[inline(always)]
	pub fn get_mut(&mut self) -> &mut T {
		self.value.get_mut()
	}
}

unsafe impl<T> Sync for Mutex<T> {}

impl<T> From<T> for Mutex<T> {
	fn from(t: T) -> Self {
		Self::new(t)
	}
}

impl<T: ~const Default> const Default for Mutex<T> {
	fn default() -> Self {
		Self::new(Default::default())
	}
}

pub struct Guard<'a, T> {
	lock: &'a Mutex<T>,
}

impl<T> Deref for Guard<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		unsafe { &*self.lock.value.get() }
	}
}

impl<T> DerefMut for Guard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { &mut *self.lock.value.get() }
	}
}

impl<T> Drop for Guard<'_, T> {
	fn drop(&mut self) {
		self.lock.lock.unlock()
	}
}
