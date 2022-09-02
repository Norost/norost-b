//! # Global allocator & memory mapping utilities.

#![no_std]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(slice_ptr_get, slice_ptr_len)]
#![deny(unsafe_op_in_unsafe_fn)]

use {
	core::{
		alloc::{self, AllocError, Allocator as IAllocator, Layout},
		ptr::{self, NonNull},
	},
	norostb_kernel::{
		syscall::{self, RWX},
		Page,
	},
};

/// An allocator that gets its memory from the OS.
///
/// All instances use the same memory pool.
pub struct Allocator;

unsafe impl alloc::Allocator for Allocator {
	#[inline]
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Keep in mind this is a *safe* function, so we have to avoid UB even if the arguments'
		// values are beyond reason.
		if layout.align() > Page::SIZE {
			// We don't support ridiculous alignment requirements.
			Err(AllocError)
		} else {
			syscall::alloc(None, Page::align_size(layout.size()), RWX::RW)
				.map(|(ptr, size)| NonNull::slice_from_raw_parts(ptr.cast(), size.get()))
				.map_err(|_| AllocError)
		}
	}

	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator, and
	/// * `layout` must [*fit*] that block of memory.
	#[inline]
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		// We allocated a page directly, so we can give it directly back to the OS.
		unsafe {
			let _r = syscall::dealloc(ptr.cast(), Page::align_size(layout.size()));
			debug_assert!(_r.is_ok(), "{:?}", _r);
		}
	}

	#[inline]
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		let mem = self.allocate(layout)?;
		// The OS already cleared the pages for us, so we don't need to clear it ourselves.
		Ok(mem)
	}

	#[inline]
	unsafe fn grow(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			old_layout.size() <= new_layout.size(),
			"new layout must be larger"
		);
		let old = Page::align_size(old_layout.size());
		let new = Page::align_size(new_layout.size());
		if old < new {
			// We need to copy & reallocate
			let new = self.allocate(new_layout)?;
			unsafe {
				new.as_ptr()
					.as_mut_ptr()
					.copy_from_nonoverlapping(ptr.as_ptr(), old_layout.size());
				self.deallocate(ptr, old_layout);
			}
			Ok(new)
		} else {
			// There is still enough room, so we don't need to do anything.
			Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
		}
	}

	#[inline]
	unsafe fn grow_zeroed(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		let ptr = unsafe { self.grow(ptr, old_layout, new_layout)? };
		let old = Page::align_size(old_layout.size());
		let new = Page::align_size(new_layout.size());
		if old < new {
			// We reallocated & the pages have already been cleared by the kernel, so no need
			// to do anything.
			Ok(ptr)
		} else {
			// We need to clear the upper part of the last page.
			unsafe {
				ptr.as_ptr()
					.as_mut_ptr()
					.add(old_layout.size())
					.write_bytes(0, new_layout.size() - old_layout.size());
			}
			Ok(ptr)
		}
	}

	#[inline]
	unsafe fn shrink(
		&self,
		ptr: NonNull<u8>,
		old_layout: Layout,
		new_layout: Layout,
	) -> Result<NonNull<[u8]>, AllocError> {
		debug_assert!(
			old_layout.size() >= new_layout.size(),
			"new layout must be smaller"
		);
		let old = Page::align_size(old_layout.size());
		let new = Page::align_size(new_layout.size());
		if new < old {
			// Give any pages we don't need back to the kernel.
			if old != new {
				unsafe {
					self.deallocate(
						NonNull::new_unchecked(ptr.as_ptr().add(new)),
						Layout::from_size_align_unchecked(old - new, Page::SIZE),
					);
				}
			}
			Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
		} else {
			// We don't need to do anything.
			Ok(NonNull::slice_from_raw_parts(ptr, new_layout.size()))
		}
	}
}

unsafe impl alloc::GlobalAlloc for Allocator {
	#[inline]
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		self.allocate(layout)
			.map_or(ptr::null_mut(), |p| p.as_ptr().as_mut_ptr())
	}

	#[inline]
	unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
		self.allocate_zeroed(layout)
			.map_or(ptr::null_mut(), |p| p.as_ptr().as_mut_ptr())
	}

	#[inline]
	unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
		debug_assert!(!ptr.is_null());
		unsafe { self.deallocate(NonNull::new_unchecked(ptr), layout) }
	}

	#[inline]
	unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
		debug_assert!(!ptr.is_null());
		let old_layout = layout;
		if let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) {
			unsafe {
				if new_layout.size() > old_layout.size() {
					self.grow(NonNull::new_unchecked(ptr), old_layout, new_layout)
						.map_or(ptr::null_mut(), |p| p.as_ptr().as_mut_ptr())
				} else {
					self.shrink(NonNull::new_unchecked(ptr), old_layout, new_layout)
						.map_or(ptr::null_mut(), |p| p.as_ptr().as_mut_ptr())
				}
			}
		} else {
			ptr::null_mut()
		}
	}
}
