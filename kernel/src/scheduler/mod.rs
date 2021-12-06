mod memory_object;
pub mod process;
pub mod syscall;
mod thread;
mod round_robin;

use core::time::Duration;
use crate::time::Monotonic;
pub use memory_object::*;
pub use thread::Thread;

pub use round_robin::count as thread_count;

/// Switch to the next thread. This does not save the current thread's state!
///
/// If no thread is scheduled, the `Monotonic` **when** the next thread becomes available is returned.
///
/// # Safety
///
/// The current thread's state must be properly saved.
pub unsafe fn next_thread() -> Result<!, Monotonic> {
	let mut thr = round_robin::next().unwrap();
	let first = thr.as_non_null_ptr();
	let now = Monotonic::now();
	dbg!(now);
	loop {
		if thr.sleep_until() <= now {
			thr.resume();
		}
		thr = round_robin::next().unwrap();
		if thr.as_non_null_ptr() == first {
			return Err(now);
		}
	}
}
