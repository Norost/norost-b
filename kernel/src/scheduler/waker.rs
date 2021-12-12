//! Waker for asynchronous operations.

use super::Thread;
use alloc::sync::Weak;
use core::task::{RawWaker, RawWakerVTable, Waker};

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

pub fn new_waker(thread: Weak<Thread>) -> Waker {
	let waker = RawWaker::new(Weak::into_raw(thread).cast(), &VTABLE);
	// SAFETY: The RawWaker is valid.
	unsafe { Waker::from_raw(waker) }
}

unsafe fn clone(thread: *const ()) -> RawWaker {
	let t = Weak::from_raw(thread.cast::<Thread>());
	let _ = Weak::into_raw(t.clone()); // Don't free the weak pointer
	RawWaker::new(Weak::into_raw(t).cast(), &VTABLE)
}

unsafe fn wake(thread: *const ()) {
	let t = Weak::from_raw(thread.cast::<Thread>());
	t.upgrade().map(|t| t.wake());
}

unsafe fn wake_by_ref(thread: *const ()) {
	let t = Weak::from_raw(thread.cast::<Thread>());
	t.upgrade().map(|t| t.wake());
	let _ = Weak::into_raw(t); // Don't free the weak pointer
}

unsafe fn drop(thread: *const ()) {
	Weak::from_raw(thread.cast::<Thread>());
}
