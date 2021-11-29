use super::frame;
use super::Page;
use core::alloc::{GlobalAlloc, Layout};
use core::num::NonZeroUsize;

#[global_allocator]
static GLOBAL: Global = Global;

struct Global;

unsafe impl GlobalAlloc for Global {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		let c = NonZeroUsize::new(Page::min_pages_for_bytes(layout.size())).unwrap();
		frame::allocate_contiguous(c)
			.unwrap()
			.as_ptr()
			.cast()
	}

	unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}

#[alloc_error_handler]
fn alloc_err_handler(_: core::alloc::Layout) -> ! {
	todo!()
}
