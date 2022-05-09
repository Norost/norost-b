use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU8, Ordering};

/// A spinlock intended for use with interrupt service routines.
///
/// This lock will disable interrupts *before* trying to acquire the lock to prevent
/// potential deadlocks if the lock is held while an IRQ is triggered.
pub struct SpinLock<T> {
	lock: AtomicU8,
	value: UnsafeCell<T>,
}

/// A guard held *outside* ISRs.
pub struct Guard<'a, T> {
	lock: &'a SpinLock<T>,
}

/// A guard held *inside* ISRs.
pub struct IsrGuard<'a, T> {
	lock: &'a SpinLock<T>,
}

impl<T> SpinLock<T> {
	pub const fn new(value: T) -> Self {
		Self {
			lock: AtomicU8::new(0),
			value: UnsafeCell::new(value),
		}
	}

	/// Lock from *outside* an ISR routine. This will disable interrupts.
	#[track_caller]
	#[inline]
	pub fn lock(&self) -> Guard<T> {
		// Ensure interrupts weren't disabled already. Re-enabling them after dropping the
		// guard could lead to bad behaviour.
		#[cfg(debug_assertions)]
		unsafe {
			let flags: usize;
			core::arch::asm!(
				"pushf",
				"pop {}",
				out(reg) flags,
			);
			assert!(
				flags & (1 << 9) != 0,
				"interrupts are disabled. If this is intended, use isr_lock()"
			);
		}
		crate::arch::disable_interrupts();
		// TODO detect double locks by same thread
		loop {
			match self
				.lock
				.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
			{
				Ok(_) => return Guard { lock: self },
				Err(_) => core::hint::spin_loop(),
			}
		}
	}

	/// Lock from *inside* an ISR routine. This will *not* disable interrupts, though
	/// they should already be disabled inside an ISR.
	#[track_caller]
	#[inline]
	pub fn isr_lock(&self) -> IsrGuard<T> {
		// Ensure interrupts aren't enabled. If they are, we're most likely not inside
		// an ISR and we also risk a deadlock.
		#[cfg(debug_assertions)]
		unsafe {
			let flags: usize;
			core::arch::asm!(
				"pushf",
				"pop {}",
				out(reg) flags,
			);
			assert!(
				flags & (1 << 9) == 0,
				"interrupts are enabled. Are we not inside an ISR?"
			);
		}
		// TODO detect double locks by same thread
		loop {
			match self
				.lock
				.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
			{
				Ok(_) => return IsrGuard { lock: self },
				Err(_) => core::hint::spin_loop(),
			}
		}
	}
}

unsafe impl<T> Sync for SpinLock<T> {}

impl<T> From<T> for SpinLock<T> {
	fn from(t: T) -> Self {
		Self::new(t)
	}
}

impl<T> Default for SpinLock<T>
where
	T: Default,
{
	fn default() -> Self {
		Self::new(Default::default())
	}
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
		ensure_interrupts_off();
		self.lock.lock.store(0, Ordering::Release);
		crate::arch::enable_interrupts();
	}
}

impl<T> Deref for IsrGuard<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		unsafe { &*self.lock.value.get() }
	}
}

impl<T> DerefMut for IsrGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { &mut *self.lock.value.get() }
	}
}

impl<T> Drop for IsrGuard<'_, T> {
	fn drop(&mut self) {
		ensure_interrupts_off();
		self.lock.lock.store(0, Ordering::Release);
	}
}

#[track_caller]
fn ensure_interrupts_off() {
	// Ensure interrupts weren't enabled in the meantime, which would lead to a potential
	// deadlock.
	#[cfg(debug_assertions)]
	unsafe {
		let flags: usize;
		core::arch::asm!(
			"pushf",
			"pop {}",
			out(reg) flags,
		);
		assert!(
			flags & (1 << 9) == 0,
			"interrupts are enabled inside ISR spinlock"
		);
	}
}
