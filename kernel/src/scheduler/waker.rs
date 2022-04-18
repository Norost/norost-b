//! Waker for asynchronous operations.

use super::Thread;
use crate::time::Monotonic;
use alloc::sync::Weak;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::task::{RawWaker, RawWakerVTable, Waker};
use core::time::Duration;

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

pub fn new_waker(thread: Weak<Thread>) -> Waker {
	let waker = RawWaker::new(Weak::into_raw(thread).cast(), &VTABLE);
	// SAFETY: The RawWaker is valid.
	unsafe { Waker::from_raw(waker) }
}

/// Get a reference to the thread of this waker if the waker contains one.
pub fn thread(waker: &Waker) -> Option<Weak<Thread>> {
	let waker = waker.as_raw();
	(waker.vtable() as *const _ == &VTABLE as *const _).then(|| {
		// SAFETY: the table is guaranteed to match since no code outside this module
		// can use the VTABLE variable directly and hence create a waker with it.
		unsafe {
			let t = Weak::from_raw(waker.data().cast::<Thread>());
			let r = t.clone();
			let _ = Weak::into_raw(t); // Don't free the weak pointer
			r
		}
	})
}

/// Asynchronously wait until a deadline is passed.
pub struct Sleep(Monotonic);

impl Sleep {
	/// Wait the given Duration from now.
	pub fn new(time: Duration) -> Self {
		Sleep(Monotonic::now().saturating_add(time))
	}
}

impl Future for Sleep {
	type Output = ();

	fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
		if self.0.duration_until(Monotonic::now()).is_some() {
			// deadline <= now
			Poll::Ready(())
		} else {
			// deadline > now
			if let Some(thr) = thread(context.waker()) {
				if let Some(thr) = Weak::upgrade(&thr) {
					thr.set_async_deadline(self.0);
				}
			} else {
				unreachable!("not a thread waker");
			}
			Poll::Pending
		}
	}
}

unsafe fn clone(thread: *const ()) -> RawWaker {
	let t = unsafe { Weak::from_raw(thread.cast::<Thread>()) };
	let _ = Weak::into_raw(t.clone()); // Don't free the weak pointer
	RawWaker::new(Weak::into_raw(t).cast(), &VTABLE)
}

unsafe fn wake(thread: *const ()) {
	let t = unsafe { Weak::from_raw(thread.cast::<Thread>()) };
	t.upgrade().map(|t| t.wake());
}

unsafe fn wake_by_ref(thread: *const ()) {
	let t = unsafe { Weak::from_raw(thread.cast::<Thread>()) };
	t.upgrade().map(|t| t.wake());
	let _ = Weak::into_raw(t); // Don't free the weak pointer
}

unsafe fn drop(thread: *const ()) {
	unsafe {
		Weak::from_raw(thread.cast::<Thread>());
	}
}
