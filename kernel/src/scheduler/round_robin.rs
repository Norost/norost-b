//! # Round-robin scheduler

use {
	super::Thread,
	crate::sync::SpinLock,
	alloc::{
		boxed::Box,
		sync::{Arc, Weak},
	},
	core::ptr::NonNull,
};

static THREAD_LIST: SpinLock<(usize, NonNull<Node>)> = SpinLock::new((0, NonNull::dangling()));

struct Node {
	next: NonNull<Node>,
	thread: Weak<Thread>,
}

pub fn insert(thread: Weak<Thread>) {
	let node = Box::new(Node { next: NonNull::new(0x1 as *mut _).unwrap(), thread });

	let mut cur_ptr = THREAD_LIST.auto_lock();
	let new = Box::leak(node);
	let new_ptr = NonNull::new(new as *mut _).unwrap();
	if cur_ptr.0 > 0 {
		let cur = unsafe { cur_ptr.1.as_mut() };
		new.next = cur.next;
		cur.next = new_ptr;
	} else {
		new.next = new_ptr;
		cur_ptr.1 = new_ptr;
	}
	cur_ptr.0 += 1;
}

/// # Note
///
/// This method should only be called inside ISRs! Internally it uses `SpinLock::isr_lock` to
/// avoid having the current thread yielded, which could result in the lock being held for
/// an excessive amount of time.
#[cfg_attr(debug_assertions, track_caller)]
#[inline]
pub fn next() -> Option<Arc<Thread>> {
	let mut l = THREAD_LIST.isr_lock();
	if l.0 == 0 {
		return None;
	}
	let mut curr = l.1;
	while l.0 > 0 {
		let nn = {
			// Use separate scope so we won't accidently use 'n' after a drop.
			// and won't have two (mutable) references to c.
			let c = unsafe { curr.as_ref() };
			let n = unsafe { c.next.as_ref() };
			if let Some(thr) = Weak::upgrade(&n.thread) {
				if !thr.destroyed() {
					l.1 = c.next;
					return Some(thr);
				}
			}
			n.next
		};
		let c = unsafe { curr.as_mut() };
		unsafe {
			let _ = Box::from_raw(c.next.as_ptr());
		}
		l.0 -= 1;
		if c.next == curr {
			return None;
		} else {
			c.next = nn;
		}
	}
	None
}
