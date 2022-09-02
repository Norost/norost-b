use {crate::thread, core::sync::atomic::Ordering};

// We use an u32 because some platforms such as RISC-V don't have native
// u8 or u16 atomic instructions. While it can be emulated it is quite a bit less efficient.
#[cfg(any(target_arch = "riscv32", target_arch = "riscv64"))]
mod atomic {
	pub use core::sync::atomic::AtomicU32 as Atomic;
	pub type AtomicVal = u32;
}
#[cfg(not(any(target_arch = "riscv32", target_arch = "riscv64")))]
mod atomic {
	pub use core::sync::atomic::AtomicU8 as Atomic;
	pub type AtomicVal = u8;
}

use atomic::*;

#[derive(Debug)]
pub struct RawMutex {
	lock: Atomic,
}

const UNLOCKED: AtomicVal = 0;
const LOCKED: AtomicVal = 1;

impl RawMutex {
	pub const fn new() -> Self {
		Self { lock: Atomic::new(UNLOCKED) }
	}

	#[inline]
	pub fn try_lock(&self) -> bool {
		self.lock
			.compare_exchange(UNLOCKED, LOCKED, Ordering::Acquire, Ordering::Relaxed)
			.is_ok()
	}

	#[inline]
	pub fn lock(&self) {
		while !self.try_lock() {
			thread::yield_now();
		}
	}

	#[inline]
	pub fn unlock(&self) {
		debug_assert_eq!(
			self.lock.load(Ordering::Relaxed),
			LOCKED,
			"unlocked during lock"
		);
		self.lock.store(UNLOCKED, Ordering::Release);
	}
}

impl Default for RawMutex {
	fn default() -> Self {
		Self::new()
	}
}
