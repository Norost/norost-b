use core::sync::atomic::{AtomicU32, Ordering};

pub struct SpinLock(AtomicU32);

impl SpinLock {
	#[inline]
	pub fn lock(&self) {
		loop {
			let n = self.0.fetch_or(1, Ordering::Acquire);
			if n == 0 {
				break;
			}
			while self.0.load(Ordering::Relaxed) != 0 {
				core::hint::spin_loop();
			}
		}
	}

	#[inline]
	pub fn unlock(&self) {
		self.0.store(0, Ordering::Release);
	}
}

pub struct Mutex(AtomicU32);

impl Mutex {
	#[inline]
	pub fn try_lock(&self) -> bool {
		self.0.fetch_or(1, Ordering::Acquire) == 0
	}

	#[inline]
	pub fn unlock(&self) {
		self.0.store(0, Ordering::Release);
	}
}
