mod memory_object;
pub mod process;
mod round_robin;
pub mod syscall;
mod thread;
mod waker;

use crate::time::Monotonic;
use crate::{arch, power};
use alloc::sync::Arc;
use core::future::Future;
use core::marker::Unpin;
use core::pin::Pin;
use core::task::{Context, Poll};
pub use memory_object::*;
pub use thread::Thread;

/// Switch to the next thread. This does not save the current thread's state!
///
/// If no thread is scheduled, the `Monotonic` **when** the next thread becomes available is
/// returned.
///
/// # Safety
///
/// The current thread's state must be properly saved.
pub unsafe fn try_next_thread() -> Result<!, Monotonic> {
	let mut thr = round_robin::next().unwrap();
	let first = Arc::as_ptr(&thr);
	let now = Monotonic::now();
	let mut t = Monotonic::MAX;
	loop {
		if thr.sleep_until() <= now {
			let _ = thr.clone().resume();
		}
		t = t.min(thr.sleep_until());
		thr = round_robin::next().unwrap();
		if Arc::as_ptr(&thr) == first {
			return Err(t);
		}
	}
}

/// Switch to the next thread. This does not save the current thread's state!
///
/// # Safety
///
/// The current thread's state must be properly saved.
pub unsafe fn next_thread() -> ! {
	use crate::driver::apic;
	loop {
		if let Err(t) = unsafe { try_next_thread() } {
			if let Some(d) = Monotonic::now().duration_until(t) {
				apic::set_timer_oneshot(d, Some(16));
				arch::enable_interrupts();
				power::halt();
			}
		}
	}
}

/// Poll a task once.
fn poll<T>(mut task: impl Future<Output = T> + Unpin) -> Poll<T> {
	let waker = waker::new_waker(Thread::current_weak().unwrap());
	Pin::new(&mut task).poll(&mut Context::from_waker(&waker))
}
