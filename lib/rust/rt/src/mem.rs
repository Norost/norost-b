use crate::{io, RWX};
use core::{num::NonZeroUsize, ptr::NonNull};
use norostb_kernel::syscall;

#[inline]
pub fn alloc(
	base: Option<NonNull<u8>>,
	size: usize,
	rwx: RWX,
) -> io::Result<(NonNull<u8>, NonZeroUsize)> {
	syscall::alloc(base.map(|p| p.cast()), size, rwx).map(|(p, s)| (p.cast(), s))
}

/// # Safety
///
/// The memory may not be accessed after this call.
#[inline]
pub unsafe fn dealloc(base: NonNull<u8>, size: usize) -> io::Result<()> {
	unsafe { syscall::dealloc(base.cast(), size) }
}
