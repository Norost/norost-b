use core::marker::PhantomData;
use core::ptr::NonNull;

#[repr(C)]
pub struct Slice<T> {
	ptr: NonNull<T>,
	len: usize,
	_marker: PhantomData<T>,
}

impl<T> Slice<T> {
	/// # Safety
	///
	/// `ptr` and `len` must be valid.
	pub unsafe fn unchecked_as_slice(&self) -> &[T] {
		core::slice::from_raw_parts(self.ptr.as_ptr(), self.len)
	}
}
