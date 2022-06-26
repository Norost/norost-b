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
		debug_assert!(
			crate::arch::interrupts_enabled(),
			"interrupts are disabled. If this is intended, use isr_lock()"
		);
		crate::arch::disable_interrupts();
		loop {
			match self
				.lock
				.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
			{
				Ok(_) => return Guard { lock: self },
				Err(_) => unreachable!("deadlock on single-core system"),
				#[allow(unreachable_patterns)]
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
		debug_assert!(
			!crate::arch::interrupts_enabled(),
			"interrupts are enabled. Are we not inside an ISR?"
		);
		self.lock_manual()
	}

	/// Lock without automatically re-enabling interrupts. It is up to the caller to ensure
	/// interrupts are *disabled* before locking!
	#[track_caller]
	#[inline]
	pub fn lock_manual(&self) -> IsrGuard<T> {
		debug_assert!(
			!crate::arch::interrupts_enabled(),
			"interrupts are enabled. Make sure interrupts are disabled!"
		);
		// TODO detect double locks by same thread
		loop {
			match self
				.lock
				.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
			{
				Ok(_) => return IsrGuard { lock: self },
				Err(_) => unreachable!("deadlock on single-core system"),
				#[allow(unreachable_patterns)]
				Err(_) => core::hint::spin_loop(),
			}
		}
	}

	/// Lock and determine automatically whether interrupts need to be re-enabled when dropping the
	/// guard.
	#[track_caller]
	#[inline]
	pub fn auto_lock(&self) -> AutoGuard<T> {
		if crate::arch::interrupts_enabled() {
			AutoGuard(AutoGuardInner::NoIsr(self.lock()))
		} else {
			AutoGuard(AutoGuardInner::Isr(self.isr_lock()))
		}
	}

	/// Borrow the lock mutably, which is safe since mutable references are always unique.
	#[allow(dead_code)]
	pub fn get_mut(&mut self) -> &mut T {
		self.value.get_mut()
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
		debug_assert_ne!(
			self.lock.lock.load(Ordering::Relaxed),
			0,
			"lock was released"
		);
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
		debug_assert_ne!(
			self.lock.lock.load(Ordering::Relaxed),
			0,
			"lock was released"
		);
		self.lock.lock.store(0, Ordering::Release);
	}
}

enum AutoGuardInner<'a, T> {
	Isr(IsrGuard<'a, T>),
	NoIsr(Guard<'a, T>),
}

/// A guard that automatically determines whether interrupts need to be re-enabled or not.
pub struct AutoGuard<'a, T>(AutoGuardInner<'a, T>);

impl<'a, T> Deref for AutoGuard<'a, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		match &self.0 {
			AutoGuardInner::Isr(t) => t,
			AutoGuardInner::NoIsr(t) => t,
		}
	}
}

impl<'a, T> DerefMut for AutoGuard<'a, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		match &mut self.0 {
			AutoGuardInner::Isr(t) => t,
			AutoGuardInner::NoIsr(t) => t,
		}
	}
}

#[track_caller]
fn ensure_interrupts_off() {
	// Ensure interrupts weren't enabled in the meantime, which would lead to a potential
	// deadlock.
	debug_assert!(
		!crate::arch::interrupts_enabled(),
		"interrupts are enabled inside ISR spinlock"
	);
}
