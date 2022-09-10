use {
	alloc::vec::Vec,
	core::{alloc::Layout, fmt, marker::PhantomData, mem, num::NonZeroUsize, ptr::NonNull},
	driver_utils::dma,
	rt::sync::Mutex,
};

static POOL_4K: Mutex<Vec<(NonNull<()>, u64)>> = Mutex::new(Vec::new());

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

impl<T> Dma<T>
where
	T: Default,
{
	pub fn new() -> Result<Self, rt::Error> {
		let s = Self::new_uninit()?;
		unsafe { s.ptr.as_ptr().write(Default::default()) };
		Ok(s)
	}
}

impl<T> Dma<T> {
	pub fn new_zeroed() -> Result<Self, rt::Error> {
		let s = Self::new_uninit()?;
		unsafe { s.ptr.as_ptr().write_bytes(0, 1) };
		Ok(s)
	}

	pub fn new_uninit() -> Result<Self, rt::Error> {
		let (ptr, phys) = alloc_raw(mem::size_of::<T>())?;
		#[cfg(feature = "poison")]
		unsafe {
			ptr.as_ptr().write_bytes(0xcc, 1)
		};
		Ok(Self { ptr: ptr.cast(), phys, _marker: PhantomData })
	}
}

impl<T> Dma<[T]>
where
	T: Default,
{
	pub fn new_slice(len: usize) -> Result<Self, rt::Error> {
		let s = Self::new_slice_uninit(len)?;
		unsafe {
			for p in s.ptr.as_uninit_slice_mut() {
				p.write(Default::default());
			}
		}
		Ok(s)
	}
}

impl<T> Dma<[T]> {
	pub fn new_slice_zeroed(len: usize) -> Result<Self, rt::Error> {
		let s = Self::new_slice_uninit(len)?;
		unsafe { s.ptr.as_ptr().as_mut_ptr().write_bytes(0, len) };
		Ok(s)
	}

	pub fn new_slice_uninit(len: usize) -> Result<Self, rt::Error> {
		let (layout, _) = Layout::new::<T>().repeat(len).unwrap();
		let (ptr, phys) = alloc_raw(layout.size())?;
		#[cfg(feature = "poison")]
		unsafe {
			ptr.as_ptr().write_bytes(0xcc, len)
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

impl<T> Drop for Dma<T>
where
	T: ?Sized,
{
	fn drop(&mut self) {
		unsafe {
			let l = mem::size_of_val(&*self.ptr.as_ptr());
			if l == 0 {
				// Do nothing
			} else if l <= 4096 {
				POOL_4K.lock().push((self.ptr.to_raw_parts().0, self.phys));
			} else {
				// TODO avoid leaking
			}
		}
	}
}

impl<T> fmt::Debug for Dma<T>
where
	T: ?Sized,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_list().entry(&format_args!("..")).finish()
	}
}

fn alloc_raw(l: usize) -> Result<(NonNull<u8>, u64), rt::Error> {
	Ok(match NonZeroUsize::new(l) {
		Some(l) => {
			let v = (l.get() <= 4096).then(|| POOL_4K.lock().pop()).flatten();
			if let Some((a, b)) = v {
				(a.cast(), b)
			} else {
				let (a, b, _) = dma::alloc_dma(l)?;
				(a, b)
			}
		}
		None => (NonNull::dangling(), 0),
	})
}
