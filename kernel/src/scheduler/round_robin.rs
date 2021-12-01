//! # Round-robin scheduler

use super::Thread;
use crate::sync::SpinLock;
use core::cell::Cell;
use core::ops::Deref;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::boxed::Box;

static THREAD_LIST: SpinLock<Option<NonNull<Node>>> = SpinLock::new(None);

struct Node {
	next: Cell<NonNull<Node>>,
	thread: Thread,
	ref_counter: AtomicUsize,
}

impl Deref for Node {
	type Target = Thread;

	fn deref(&self) -> &Self::Target {
		&self.thread
	}
}

pub struct Guard {
	node: NonNull<Node>,
}

impl Guard {
	/// # Safety
	///
	/// The node must be valid.
	unsafe fn new(node: NonNull<Node>) -> Self {
		node.as_ref().ref_counter.fetch_add(1, Ordering::Relaxed);
		Self { node }
	}
}

impl Drop for Guard {
	fn drop(&mut self) {
		// SAFETY: the node is valid.
		let n = unsafe { self.node.as_ref() };
		if n.ref_counter.fetch_sub(1, Ordering::Relaxed) == 0 {
			// SAFETY: the node was allocated as a Box.
			unsafe {
				Box::from_raw(self.node.as_ptr());
			}
		}
	}
}

impl Deref for Guard {
	type Target = Thread;

	fn deref(&self) -> &Self::Target {
		// SAFETY: the node is valid.
		unsafe {
			self.node.as_ref()
		}
	}
}

pub fn insert(thread: Thread) {
	let node = Box::new(Node {
		next: Cell::new(NonNull::new(0x1 as *mut _).unwrap()),
		thread,
		ref_counter: AtomicUsize::new(0),
	});
	let mut n = THREAD_LIST.lock();
	let node = Box::leak(node);
	let ptr = NonNull::new(node as *mut _).unwrap();
	if let Some(n) = *n {
		let n = unsafe { n.as_ref() };
		node.next.set(n.next.get());
		n.next.set(ptr);
	} else {
		node.next.set(ptr);
		*n = Some(ptr);
	}
}

pub fn next() -> Option<Guard> {
	let mut l = THREAD_LIST.lock();
	l.map(|n| unsafe {
		*l = Some(n.as_ref().next.get());
		Guard::new(n)
	})
}
