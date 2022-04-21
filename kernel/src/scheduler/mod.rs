mod memory_object;
pub mod process;
mod round_robin;
pub mod syscall;
mod thread;
mod waker;

use crate::time::Monotonic;
use alloc::sync::{Arc, Weak};
use arena::Arena;
use core::future::Future;
use core::marker::Unpin;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;
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
	let first = Arc::as_ptr(&thr);
	let now = Monotonic::now();
	let mut t = Monotonic::MAX;
	loop {
		if thr.sleep_until() <= now {
			thr.resume();
		}
		t = t.min(thr.sleep_until());
		thr = round_robin::next().unwrap();
		if Arc::as_ptr(&thr) == first {
			return Err(t);
		}
	}
}

/// Wait for an asynchronous task to finish.
fn block_on<T>(mut task: impl Future<Output = T> + Unpin) -> T {
	let waker = waker::new_waker(Thread::current_weak());
	let mut context = Context::from_waker(&waker);
	loop {
		if let Poll::Ready(res) = Pin::new(&mut task).poll(&mut context) {
			return res;
		}
		Weak::upgrade(&waker::thread(&waker).expect("waker type changed"))
			.expect("no thread")
			.sleep(Duration::MAX);
	}
}

/// Wait for an asynchronous task to finish or until the timeout expires.
fn block_on_timeout<T>(
	mut task: impl Future<Output = T> + Unpin,
	timeout: Duration,
) -> Result<T, ()> {
	let waker = waker::new_waker(Thread::current_weak());
	let mut context = Context::from_waker(&waker);
	if let Poll::Ready(res) = Pin::new(&mut task).poll(&mut context) {
		return Ok(res);
	}
	let mut sleep = waker::Sleep::new(timeout);
	loop {
		if let Poll::Ready(()) = Pin::new(&mut sleep).poll(&mut context) {
			return Err(());
		}
		if let Poll::Ready(res) = Pin::new(&mut task).poll(&mut context) {
			return Ok(res);
		}
		Weak::upgrade(&waker::thread(&waker).expect("waker type changed"))
			.expect("no thread")
			.sleep(Duration::MAX);
	}
}
