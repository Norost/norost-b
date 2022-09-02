use {
	core::{alloc::Layout, marker::PhantomData, mem, ptr::NonNull},
	driver_utils::dma,
};

pub struct Dma<T>
where
	T: ?Sized,
{
	ptr: NonNull<T>,
	phys: u64,
	_marker: PhantomData<T>,
}

impl<T> Dma<T>
where
	T: ?Sized,
{
	pub unsafe fn as_ref(&self) -> &T {
		self.ptr.as_ref()
	}

	pub unsafe fn as_mut(&mut self) -> &mut T {
		self.ptr.as_mut()
	}

	#[allow(dead_code)]
	pub fn as_ptr(&self) -> NonNull<T> {
		self.ptr
	}

	pub fn as_phys(&self) -> u64 {
		self.phys
	}

	pub fn into_raw(self) -> (NonNull<T>, u64) {
		let s = mem::ManuallyDrop::new(self);
		(s.ptr, s.phys)
	}
}

impl<T> Dma<T> {
	pub fn new() -> Result<Self, rt::Error> {
		let (ptr, phys) = match mem::size_of::<T>().try_into() {
			Ok(l) => {
				let (a, b, _) = dma::alloc_dma(l)?;
				(a, b)
			}
			Err(_) => (NonNull::dangling(), 0),
		};
		Ok(Self { ptr: ptr.cast(), phys, _marker: PhantomData })
	}
}

impl<T> Dma<[T]> {
	pub fn new_slice(len: usize) -> Result<Self, rt::Error> {
		let (layout, _) = Layout::new::<T>().repeat(len).unwrap();
		let (ptr, phys) = match layout.size().try_into() {
			Ok(l) => {
				let (a, b, _) = dma::alloc_dma(l)?;
				(a, b)
			}
			Err(_) => (NonNull::dangling(), 0),
		};
		Ok(Self {
			ptr: NonNull::slice_from_raw_parts(ptr.cast(), len),
			phys,
			_marker: PhantomData,
		})
	}

	pub fn len(&self) -> usize {
		self.ptr.len()
	}
}
