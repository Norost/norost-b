#[cfg(feature = "debug-sanitize-heap")]
mod sanitizer;

#[cfg(not(feature = "debug-sanitize-heap"))]
mod default {
	use super::super::{
		frame::{self, AllocateHints, OwnedPageFrames, PPN},
		r#virtual::{AddressSpace, RWX},
		Page,
	};
	use alloc::sync::Arc;
	use core::alloc::{GlobalAlloc, Layout};
	use core::num::NonZeroUsize;
	use core::ptr::{self, NonNull};

	#[global_allocator]
	static GLOBAL: Global = Global;

	struct Global;

	unsafe impl GlobalAlloc for Global {
		unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
			if layout.align() > Page::SIZE {
				ptr::null_mut()
			} else if let Some(c) = NonZeroUsize::new(Page::min_pages_for_bytes(layout.size())) {
				if c.get() > 1 {
					let frames = Arc::new(
						OwnedPageFrames::new(
							c,
							AllocateHints {
								address: 0 as _,
								color: 0,
							},
						)
						.unwrap(),
					);
					AddressSpace::kernel_map_object(None, frames, RWX::RW)
						.unwrap()
						.0
						.as_ptr()
						.cast()
				} else {
					let mut f = None;
					frame::allocate(c.get(), |e| f = Some(e), 0 as _, 0).unwrap();
					f.unwrap().as_ptr().cast()
				}
			} else {
				ptr::null_mut()
			}
		}

		// No alloc_zeroed as there is nothing to optimize to it.

		unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
			let c = Page::min_pages_for_bytes(layout.size());
			let c = NonZeroUsize::new(c).unwrap();
			if c.get() > 1 {
				let ptr = NonNull::new(ptr).unwrap();
				unsafe {
					AddressSpace::kernel_unmap_object(ptr.cast(), c).unwrap();
				}
			} else {
				unsafe { frame::deallocate(c.get(), || PPN::from_ptr(ptr.cast::<Page>())) }
					.unwrap();
			}
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
						// SAFETY: we're only copying the minimum amount of bytes necessary,
						// which is guaranteed to fit and won't break our mind when it overflows
						// into pages it shouldn't.
						let count = new_layout.size().min(old_layout.size());
						new_ptr.copy_from_nonoverlapping(ptr, count);
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
