#[cfg(feature = "debug-sanitize-heap")]
mod sanitizer;

#[cfg(not(feature = "debug-sanitize-heap"))]
mod default {
	use super::super::{
		frame::{self, PPN},
		Page,
	};
	use core::alloc::{GlobalAlloc, Layout};
	use core::num::NonZeroUsize;
	use core::ptr;

	#[global_allocator]
	static GLOBAL: Global = Global;

	struct Global;

	unsafe impl GlobalAlloc for Global {
		unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
			if layout.align() > Page::SIZE {
				ptr::null_mut()
			} else if let Some(c) = NonZeroUsize::new(Page::min_pages_for_bytes(layout.size())) {
				frame::allocate_contiguous(c).unwrap().as_ptr().cast()
			} else {
				ptr::null_mut()
			}
		}

		// No alloc_zeroed as there is nothing to optimize to it.

		unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
			let c = Page::min_pages_for_bytes(layout.size());
			let mut base = unsafe { PPN::from_ptr(ptr.cast::<Page>()) };
			unsafe {
				frame::deallocate(c, || {
					let f = base;
					base = base.next();
					f
				})
			}
			.unwrap();
		}

		unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
			let old_c = NonZeroUsize::new(Page::min_pages_for_bytes(old_layout.size()))
				.expect("old size must be greater than 0");
			let new_c = NonZeroUsize::new(Page::min_pages_for_bytes(new_size))
				.expect("new size must be greater than 0");
			if old_c == new_c {
				// No need to reallocate, as there is still space left.
				ptr
			} else {
				// We need to reallocate.
				let new_layout = Layout::from_size_align(new_size, old_layout.align()).unwrap();
				unsafe {
					let new_ptr = self.alloc(new_layout);
					if new_ptr != ptr::null_mut() {
						new_ptr.copy_from_nonoverlapping(ptr, old_layout.size());
						self.dealloc(ptr, old_layout);
					}
					new_ptr
				}
			}
		}
	}
}

#[alloc_error_handler]
fn alloc_err_handler(_: core::alloc::Layout) -> ! {
	todo!()
}
