use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock(AtomicBool);

impl SpinLock {
	#[inline]
	pub fn lock(&self) {
		loop {
			if self
				.0
				.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
				.is_ok()
			{
				return;
			}
			while self.0.load(Ordering::Relaxed) {
				core::hint::spin_loop();
			}
		}
	}

	#[inline]
	pub fn unlock(&self) {
		self.0.store(false, Ordering::Release);
	}
}

impl const Default for SpinLock {
	fn default() -> Self {
		Self(AtomicBool::default())
	}
}

pub struct Mutex(AtomicBool);

impl Mutex {
	#[inline]
	pub fn try_lock(&self) -> bool {
		self.0
			.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
			.is_ok()
	}

	#[inline]
	pub fn unlock(&self) {
		self.0.store(false, Ordering::Release);
	}
}

impl const Default for Mutex {
	fn default() -> Self {
		Self(AtomicBool::default())
	}
}
