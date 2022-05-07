//! 64KB bump allocator.

use core::alloc::Layout;

#[repr(align(4096))]
struct Page([u8; 4096]);

static mut BUF: [Page; 16] = [const { Page([0; 4096]) }; 16];
static mut USED: u16 = 0;

pub fn buffer_ptr() -> *mut u8 {
	unsafe { BUF.as_mut_ptr().cast() }
}

pub fn alloc(layout: Layout) -> *mut u8 {
	unsafe {
		// First align buffer
		let new_used = usize::from(USED);
		let new_used = (new_used + layout.align() - 1) & !(layout.align() - 1);
		let ptr = BUF.as_mut_ptr().cast::<u8>().add(new_used);
		// Then reserve space
		let new_used = new_used + layout.size();
		USED = new_used.try_into().expect("out of memory");
		ptr
	}
}

pub fn offset(n: *const u8) -> u16 {
	(n as usize - buffer_ptr() as usize).try_into().unwrap()
}
