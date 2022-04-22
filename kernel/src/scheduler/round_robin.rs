//! # Round-robin scheduler

use super::Thread;
use crate::sync::SpinLock;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
};
use core::ptr::NonNull;

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
	let mut cur_ptr = THREAD_LIST.lock();
	let new = Box::leak(node);
	let new_ptr = NonNull::new(new as *mut _).unwrap();
	if let Some(mut cur_ptr) = cur_ptr.1 {
		let cur = unsafe { cur_ptr.as_mut() };
		new.next = cur.next;
		cur.next = new_ptr;
	} else {
		new.next = new_ptr;
		cur_ptr.1 = Some(new_ptr);
	}
	cur_ptr.0 += 1;
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
				if !thr.destroyed() {
					l.1 = Some(c.next);
					return Some(thr);
				}
			}
			n.next
		};
		let c = unsafe { curr.as_mut() };
		drop(unsafe {
			Box::from_raw(c.next.as_ptr());
		});
		if c.next == curr {
			l.1 = None;
			return None;
		} else {
			c.next = nn;
		}
	}
}
