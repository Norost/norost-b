use core::{
	mem::ManuallyDrop,
	ops::{Deref, DerefMut},
};

/// A spinlock intended for use with interrupt service routines.
///
/// This lock will disable interrupts *before* trying to acquire the lock to prevent
/// potential deadlocks if the lock is held while an IRQ is triggered.
#[derive(Default)]
pub struct IsrSpinLock<T> {
	inner: super::spinlock::SpinLock<T>,
}

/// A guard held *outside* ISRs.
pub struct Guard<'a, T> {
	inner: ManuallyDrop<super::spinlock::Guard<'a, T>>,
}

/// A guard held *inside* ISRs.
pub struct IsrGuard<'a, T> {
	inner: super::spinlock::Guard<'a, T>,
}

impl<T> IsrSpinLock<T> {
	pub const fn new(value: T) -> Self {
		Self {
			inner: super::spinlock::SpinLock::new(value),
		}
	}

	/// Lock from *outside* an ISR routine. This will disable interrupts.
	#[track_caller]
	pub fn lock(&self) -> Guard<T> {
		crate::arch::disable_interrupts();
		Guard {
			inner: ManuallyDrop::new(self.inner.lock()),
		}
	}

	/// Lock from *inside* an ISR routine. This will *not* disable interrupts, though
	/// they should already be disabled inside an ISR.
	#[track_caller]
	pub fn isr_lock(&self) -> IsrGuard<T> {
		IsrGuard {
			inner: self.inner.lock(),
		}
	}
}

impl<T> From<T> for IsrSpinLock<T> {
	fn from(t: T) -> Self {
		Self::new(t)
	}
}

impl<T> Deref for Guard<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&*self.inner
	}
}

impl<T> DerefMut for Guard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut *self.inner
	}
}

impl<T> Drop for Guard<'_, T> {
	fn drop(&mut self) {
		// SAFETY: we only drop inner once here
		unsafe { ManuallyDrop::drop(&mut self.inner) };
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
			assert_eq!(
				flags & (1 << 9),
				0,
				"interrupts are enabled inside ISR spinlock"
			);
		}
		crate::arch::enable_interrupts();
	}
}

impl<T> Deref for IsrGuard<'_, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		&*self.inner
	}
}

impl<T> DerefMut for IsrGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut *self.inner
	}
}
