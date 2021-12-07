//! # Round-robin scheduler

use super::Thread;
use crate::sync::SpinLock;
use core::ops::Deref;
use core::ptr::NonNull;
use alloc::{boxed::Box, sync::{Arc, Weak}};

static THREAD_LIST: SpinLock<(usize, Option<NonNull<Node>>)> = SpinLock::new((0, None));

struct Node {
	next: NonNull<Node>,
	thread: Weak<Thread>,
}

pub fn insert(thread: Weak<Thread>) {
	let node = Box::new(Node {
		next: NonNull::new(0x1 as *mut _).unwrap(),
		thread,
	});
	let mut n = THREAD_LIST.lock();
	let node = Box::leak(node);
	let ptr = NonNull::new(node as *mut _).unwrap();
	if let Some(mut n) = n.1 {
		let n = unsafe { n.as_mut() };
		node.next = n.next;
		n.next = ptr;
	} else {
		node.next = ptr;
		n.1 = Some(ptr);
	}
	n.0 += 1;
}

pub fn next() -> Option<Arc<Thread>> {
	let mut l = THREAD_LIST.lock();
	let mut curr = l.1?;
	loop {
		let nn = {
			// Use separate scope so we won't accidently use 'n' after a drop.
			// and won't have two (mutable) references to c.
			let c = unsafe { curr.as_ref() };
			let n = unsafe { c.next.as_ref() };
			if let Some(thr) = Weak::upgrade(&n.thread) {
				l.1 = Some(c.next);
				return Some(thr);
			}
			n.next
		};
		let c = unsafe { curr.as_mut() };
		drop(unsafe { Box::from_raw(c.next.as_ptr()); });
		if c.next == curr {
			l.1 = None;
			return None;
		} else {
			c.next = nn;
		}
	}
}

pub fn count() -> usize {
	THREAD_LIST.lock().0
}
