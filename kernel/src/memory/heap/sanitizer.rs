//! An allocator intended for debugging those pesky use-after-frees and other nasal demons.

use super::super::frame::{self, PageFrame, PPN};
use super::super::Page;
use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;
use core::ptr;

#[global_allocator]
static GLOBAL: Global = Global;

struct Global;

// Allocation tracker to find invalid frees.
static mut ALLOC_TRACKER: [(*mut u8, usize); 4096] = [(0 as *mut _, 0); 4096];

// Deallocation tracker to find use-after-frees.
static mut DEALLOC_TRACKER: [(*mut u8, usize); 4096] = [(0 as *mut _, 0); 4096];

// Poison value to find unitialized reads & use-after-frees.
const POISON_VALUE: u8 = 0x42;

fn track_alloc(ptr: *mut u8, size: usize) {
	unsafe {
		assert!(
			ALLOC_TRACKER.iter().all(|e| e.0 != ptr),
			"double alloc (frame allocator bug)"
		);
		*ALLOC_TRACKER
			.iter_mut()
			.find(|e| e.0 == ptr::null_mut())
			.unwrap() = (ptr, size)
	}
}

fn untrack_alloc(ptr: *mut u8, size: usize) {
	unsafe {
		if let Some(e) = ALLOC_TRACKER.iter_mut().find(|e| e.0 == ptr) {
			e.0 = 0 as *mut _;
		} else {
			panic!("free of invalid pointer");
		}
	}
}

fn track_dealloc(ptr: *mut u8, size: usize) {
	unsafe {
		assert!(DEALLOC_TRACKER.iter().all(|e| e.0 != ptr), "double free");
		*DEALLOC_TRACKER
			.iter_mut()
			.find(|e| e.0 == ptr::null_mut())
			.unwrap() = (ptr, size)
	}
}

fn untrack_dealloc(ptr: *mut u8) {
	unsafe {
		if let Some(e) = DEALLOC_TRACKER.iter_mut().find(|e| e.0 == ptr) {
			e.0 = 0 as *mut _;
		}
	}
}

fn poison(ptr: *mut u8, size: usize) {
	unsafe { ptr.write_bytes(POISON_VALUE, size) }
}

fn assert_poisoned(ptr: *mut u8, size: usize) {
	unsafe {
		assert!(core::slice::from_raw_parts(ptr, size)
			.iter()
			.all(|&e| e == POISON_VALUE));
	}
}

fn assert_dealloc_poisoned() {
	unsafe {
		for &(ptr, size) in DEALLOC_TRACKER.iter().filter(|e| e.0 != ptr::null_mut()) {
			assert_poisoned(ptr, size);
		}
	}
}

unsafe impl GlobalAlloc for Global {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		assert_dealloc_poisoned();

		if layout.align() > Page::SIZE {
			ptr::null_mut()
		} else if let Some(c) = NonZeroUsize::new(Page::min_pages_for_bytes(layout.size())) {
			let ptr = frame::allocate_contiguous(c).unwrap().as_ptr().cast();

			untrack_dealloc(ptr);
			poison(ptr, c.get() * Page::SIZE);
			track_alloc(ptr, layout.size());

			ptr
		} else {
			ptr::null_mut()
		}
	}

	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		assert_dealloc_poisoned();

		let c = Page::min_pages_for_bytes(layout.size());

		untrack_alloc(ptr, layout.size());
		poison(ptr, c * Page::SIZE);
		track_dealloc(ptr, layout.size());
	}
}
