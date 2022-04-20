#[cfg(feature = "debug-sanitize-heap")]
mod sanitizer;

#[cfg(not(feature = "debug-sanitize-heap"))]
mod default {
	use super::super::{
		frame::{self, PageFrame, PPN},
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

		unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
			let c = Page::min_pages_for_bytes(layout.size());
			let mut base = unsafe { PPN::from_ptr(ptr.cast::<Page>()) };
			unsafe {
				frame::deallocate(c, || {
					let f = PageFrame { base, p2size: 0 };
					base = base.next();
					f
				})
			}
			.unwrap();
		}
	}
}

#[alloc_error_handler]
fn alloc_err_handler(_: core::alloc::Layout) -> ! {
	todo!()
}
