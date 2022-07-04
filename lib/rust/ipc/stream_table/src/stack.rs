use core::{
	ptr::NonNull,
	sync::atomic::{AtomicU32, Ordering},
};

pub unsafe fn push(head: &AtomicU32, base: NonNull<u8>, offset: u32, block_size: u32) {
	debug_assert!(base.as_ptr() as usize & 0x3 == 0);
	debug_assert!(block_size.count_ones() == 1);
	let mut cur = head.load(Ordering::Relaxed);
	loop {
		let o = offset as usize * block_size as usize;
		(&*base.as_ptr().add(o).cast::<AtomicU32>()).store(cur, Ordering::Relaxed);
		match head.compare_exchange_weak(cur, offset, Ordering::Relaxed, Ordering::Relaxed) {
			Ok(_) => break,
			Err(c) => cur = c,
		}
	}
}

pub unsafe fn pop(head: &AtomicU32, base: NonNull<u8>, block_size: u32) -> Option<u32> {
	debug_assert!(base.as_ptr() as usize & 0x3 == 0);
	debug_assert!(block_size.count_ones() == 1);
	let mut cur = head.load(Ordering::Acquire);
	if cur == u32::MAX {
		return None;
	}
	loop {
		let o = cur as usize * block_size as usize;
		let next = (&*base.as_ptr().add(o).cast::<AtomicU32>()).load(Ordering::Relaxed);
		match head.compare_exchange_weak(cur, next, Ordering::Relaxed, Ordering::Relaxed) {
			Ok(_) => break,
			Err(c) => cur = c,
		}
	}
	Some(cur)
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn push_pop_0() {
		let mut buf = [0u32; 64];
		let head = AtomicU32::new(u32::MAX);
		unsafe {
			push(&head, NonNull::from(&mut buf).cast(), 0);
			assert_eq!(head.load(Ordering::Relaxed), 0);
			assert_eq!(buf[0], u32::MAX);
			pop(&head, NonNull::from(&mut buf).cast());
			assert_eq!(head.load(Ordering::Relaxed), u32::MAX);
		}
	}

	#[test]
	fn push_pop_4() {
		let mut buf = [0u32; 64];
		let head = AtomicU32::new(u32::MAX);
		unsafe {
			push(&head, NonNull::from(&mut buf).cast(), 4);
			assert_eq!(head.load(Ordering::Relaxed), 4);
			assert_eq!(buf[1], u32::MAX);
			pop(&head, NonNull::from(&mut buf).cast());
			assert_eq!(head.load(Ordering::Relaxed), u32::MAX);
		}
	}

	#[test]
	fn push_push_pop_pop_0_4() {
		let mut buf = [0u32; 64];
		let head = AtomicU32::new(u32::MAX);
		unsafe {
			push(&head, NonNull::from(&mut buf).cast(), 0);
			assert_eq!(head.load(Ordering::Relaxed), 0);
			assert_eq!(buf[0], u32::MAX);
			push(&head, NonNull::from(&mut buf).cast(), 4);
			assert_eq!(head.load(Ordering::Relaxed), 4);
			assert_eq!(buf[0], u32::MAX);
			assert_eq!(buf[1], 0);
			pop(&head, NonNull::from(&mut buf).cast());
			assert_eq!(head.load(Ordering::Relaxed), 0);
			assert_eq!(buf[0], u32::MAX);
			pop(&head, NonNull::from(&mut buf).cast());
			assert_eq!(head.load(Ordering::Relaxed), u32::MAX);
		}
	}
}
