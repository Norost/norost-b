use core::{marker::PhantomData, mem, ptr::NonNull};
use driver_utils::dma;

pub struct Dma<T> {
	ptr: NonNull<T>,
	phys: u64,
	_marker: PhantomData<T>,
}

impl<T> Dma<T> {
	pub fn new() -> Result<Self, rt::Error> {
		let (ptr, phys, _) = dma::alloc_dma(mem::size_of::<T>().try_into().unwrap())?;
		Ok(Self {
			ptr: ptr.cast(),
			phys,
			_marker: PhantomData,
		})
	}

	pub unsafe fn as_mut(&mut self) -> &mut T {
		self.ptr.as_mut()
	}

	pub fn as_ptr(&self) -> NonNull<T> {
		self.ptr
	}

	pub fn as_phys(&self) -> u64 {
		self.phys
	}
}
