mod memory_object;
pub mod process;
mod round_robin;
pub mod syscall;
mod thread;
mod waker;

use crate::arch;
use crate::time::Monotonic;
use alloc::sync::Arc;
use core::future::Future;
use core::marker::Unpin;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::task::{Context, Poll};
pub use memory_object::*;
pub use thread::Thread;

static mut SLEEP_THREADS: [MaybeUninit<Arc<Thread>>; 1] = MaybeUninit::uninit_array();

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
		let sleep_until = thr.sleep_until();
		if sleep_until <= now {
			// Be very careful _not_ to clone here, as otherwise we'll start leaking references.
			let _ = thr.resume();
		}
		t = t.min(sleep_until);
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
				apic::set_timer_oneshot(d);
				unsafe {
					SLEEP_THREADS[0].assume_init_ref().clone().resume().unwrap();
				}
			}
		}
	}
}

/// Poll a task once.
fn poll<T>(mut task: impl Future<Output = T> + Unpin) -> Poll<T> {
	let waker = waker::new_waker(Thread::current_weak().unwrap());
	Pin::new(&mut task).poll(&mut Context::from_waker(&waker))
}

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init(root: &crate::object_table::Root) {
	process::init(root);
	unsafe {
		for t in SLEEP_THREADS.iter_mut() {
			t.write(
				Thread::kernel_new(halt_forever)
					.expect("failed to create sleep thread")
					.into(),
			);
		}
	}
}

/// Halt forever. Used to implement sleep.
extern "C" fn halt_forever() -> ! {
	loop {
		arch::halt();
		arch::yield_current_thread();
	}
}
