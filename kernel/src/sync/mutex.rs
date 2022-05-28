use crate::scheduler::Thread;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::{
	ptr,
	sync::atomic::{AtomicPtr, Ordering},
};

/// A very basic spinlock implementation. Intended for short sections that are mostly uncontended.
pub struct Mutex<T> {
	lock: AtomicPtr<Thread>,
	value: UnsafeCell<T>,
}

impl<T> Mutex<T> {
	pub const fn new(value: T) -> Self {
		Self {
			lock: AtomicPtr::new(ptr::null_mut()),
			value: UnsafeCell::new(value),
		}
	}

	#[track_caller]
	pub fn lock(&self) -> Guard<T> {
		// Mutexes may never be locked inside an ISR since it can lead to deadlocks.
		debug_assert!(
			crate::arch::interrupts_enabled(),
			"interrupts are disabled. Is the mutex being locked inside an ISR?"
		);
		let thread = Thread::current_ptr()
			.expect("a mutex cannot be locked outside a thread")
			.as_ptr();
		loop {
			match self.lock.compare_exchange(
				ptr::null_mut(),
				thread,
				Ordering::Acquire,
				Ordering::Relaxed,
			) {
				Ok(_) => return Guard { lock: self },
				Err(cur) => {
					assert_ne!(thread, cur, "deadlock: same thread acquired lock twice");
					Thread::yield_current();
				}
			}
		}
	}

	/// Borrow the lock mutably, which is safe since mutable references are always unique.
	#[allow(dead_code)]
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
		debug_assert_eq!(
			self.lock.lock.load(Ordering::Relaxed),
			Thread::current_ptr().unwrap().as_ptr(),
			"lock is not held by this thread"
		);
		self.lock.lock.store(ptr::null_mut(), Ordering::Release);
	}
}
