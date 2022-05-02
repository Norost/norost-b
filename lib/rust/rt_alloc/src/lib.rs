//! # Global allocator & memory mapping utilities.

#![no_std]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(slice_ptr_get, slice_ptr_len)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::alloc::{self, AllocError, Allocator as IAllocator, Layout};
use core::mem;
use core::ptr::{self, NonNull};
use norostb_kernel::syscall::{self, RWX};

/// An allocator that gets its memory from the OS.
///
/// All instances use the same memory pool.
pub struct Allocator;

/// Whether we should allocate pages directly from the os or rely on slabmalloc
/// for the given layout.
fn should_alloc_pages(layout: Layout) -> bool {
	layout.size() >= mem::size_of::<norostb_kernel::Page>() || true
}

/// Allocate a range of pages directly from the kernel.
fn alloc_pages(layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
	syscall::alloc(None, layout.size(), RWX::RW)
		.map(|(ptr, size)| NonNull::slice_from_raw_parts(ptr.cast(), size.get()))
		.map_err(|_| AllocError)
}

/// Give back a range of pages to the kernel.
fn dealloc_pages(ptr: NonNull<u8>, size: usize) {
	unsafe {
		syscall::dealloc(ptr.cast(), size, false, true).expect("failed to deallocate");
	}
}

unsafe impl alloc::Allocator for Allocator {
	#[inline]
	fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		// Keep in mind this is a *safe* function, so we have to avoid UB even if the arguments'
		// values are beyond reason.
		if layout.align() >= mem::size_of::<norostb_kernel::Page>() {
			unsafe {
				core::arch::asm!("ud2");
			}
			// We don't support ridiculous alignment requirements.
			Err(AllocError)
		} else if should_alloc_pages(layout) {
			// If the allocation is larger than a single page, then just allocate the required
			// pages directly.
			alloc_pages(layout)
		} else {
			// For smaller allocations, use slabmalloc so we don't potentially waste a huge
			// amount of memory.
			todo!()
		}
	}

	/// # Safety
	///
	/// * `ptr` must denote a block of memory [*currently allocated*] via this allocator, and
	/// * `layout` must [*fit*] that block of memory.
	#[inline]
	unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
		// Layout should fit the memory block pointed at by ptr, so no extra safety checks should
		// be necessary.
		if should_alloc_pages(layout) {
			// We allocated a page directly, so we can give it directly back to the OS.
			dealloc_pages(ptr, layout.size())
		} else {
			todo!()
		}
	}

	#[inline]
	fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
		let mem = self.allocate(layout)?;
		if should_alloc_pages(layout) {
			// The OS already cleared the pages for us, so we don't need to clear it ourselves.
			Ok(mem)
		} else {
			// We allocated from slabmalloc, which doesn't clear memory for us.
			// SAFETY: the pointer we got from allocate() is valid.
			unsafe {
				mem.as_ptr().as_mut_ptr().write_bytes(0, mem.len());
			}
			Ok(mem)
		}
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
		if !should_alloc_pages(old_layout) && should_alloc_pages(new_layout)
		//|| ZoneAllocator::get_max_size(old_layout.size())
		//	.map_or(false, |s| s >= new_layout.size())
		{
			// We need to either:
			// - copy from slabmalloc to directly allocated pages.
			// - copy from old pages to new pages because we don't know if anything has
			//   been allocated beyond the old pages (which is something we should keep
			//   track of, probably...)
			let new = alloc_pages(new_layout)?;
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
		if should_alloc_pages(new_layout) {
			// The pages have already been cleared by the kernel, so no need to do anything.
			Ok(ptr)
		} else {
			// slabmalloc doesn't clear memory.
			unsafe {
				ptr.as_ptr()
					.as_mut_ptr()
					.wrapping_add(old_layout.size())
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
		if should_alloc_pages(old_layout) && !should_alloc_pages(new_layout)
		//|| ZoneAllocator::get_max_size(old_layout.size())
		//	.map_or(false, |s| s >= new_layout.size())
		{
			// We need to copy from directly allocated pages to slabmalloc.
			let new = alloc_pages(new_layout)?;
			unsafe {
				new.as_ptr()
					.as_mut_ptr()
					.copy_from_nonoverlapping(ptr.as_ptr(), old_layout.size());
				self.deallocate(ptr, old_layout);
			}
			Ok(new)
		} else if should_alloc_pages(new_layout) {
			// Give any pages we don't need back to the kernel.
			let size = new_layout.size() - old_layout.size();
			dealloc_pages(
				NonNull::new(ptr.as_ptr().wrapping_add(old_layout.size())).unwrap(),
				size,
			);
			Ok(NonNull::slice_from_raw_parts(ptr, size))
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
