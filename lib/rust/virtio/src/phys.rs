//! Utilities to deal with physical addresses.

use core::{
	fmt,
	marker::PhantomData,
	mem,
	ops::{Add, Sub},
	ptr::{self, NonNull},
	slice,
};
use endian::u64le;

/// Representation of a physical address.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PhysAddr(pub u64le);

impl PhysAddr {
	pub fn new(n: u64) -> Self {
		Self(n.into())
	}
}

impl Add<u64> for PhysAddr {
	type Output = Self;

	fn add(self, rhs: u64) -> Self::Output {
		Self((u64::from(self.0) + rhs).into())
	}
}

impl Sub<u64> for PhysAddr {
	type Output = Self;

	fn sub(self, rhs: u64) -> Self::Output {
		Self((u64::from(self.0) + rhs).into())
	}
}

impl fmt::Debug for PhysAddr {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:#x}", self.0)
	}
}

#[derive(Clone, Copy)]
pub struct PhysRegion {
	pub base: PhysAddr,
	pub size: u32,
}

pub struct PhysMap<'a> {
	virt: NonNull<u8>,
	phys: PhysAddr,
	size: usize,
	_marker: PhantomData<&'a mut u8>,
}

impl<'a> PhysMap<'a> {
	/// # Safety
	///
	/// `virt` must map to `phys` and `size` must be valid.
	///
	/// The pointer must be unique, i.e. there may not be any active references to the memory
	/// region.
	///
	/// The lifetime must be valid.
	#[inline(always)]
	pub unsafe fn new(virt: NonNull<u8>, phys: PhysAddr, size: usize) -> Self {
		Self {
			virt,
			phys,
			size,
			_marker: PhantomData,
		}
	}

	#[inline(always)]
	pub fn virt(&self) -> NonNull<u8> {
		self.virt
	}

	#[inline(always)]
	pub fn phys(&self) -> PhysAddr {
		self.phys
	}

	#[inline(always)]
	pub fn size(&self) -> usize {
		self.size
	}

	/// Split the buffer at a specific point.
	///
	/// # Panics
	///
	/// The index is out of range.
	#[track_caller]
	#[inline(always)]
	pub fn split_at(&mut self, index: usize) -> (Self, Self) {
		self.try_split_at(index).expect("failed to split")
	}

	/// Try to split the buffer at a specific point. Returns an error if the index is out of range.
	pub fn try_split_at(&mut self, index: usize) -> Result<(Self, Self), BufferTooSmall> {
		if self.size < index {
			Err(BufferTooSmall)
		} else {
			Ok((
				Self {
					virt: self.virt,
					phys: self.phys,
					size: index,
					_marker: self._marker,
				},
				Self {
					// This should never overflow if the contract in Self::new() was upheld
					virt: NonNull::new(self.virt.as_ptr().wrapping_add(index)).unwrap(),
					phys: self.phys + u64::try_from(index).unwrap(),
					size: self.size - index,
					_marker: self._marker,
				},
			))
		}
	}

	/// Copy data to this buffer.
	///
	/// # Panics
	///
	/// The buffer is too small.
	#[track_caller]
	#[inline(always)]
	pub fn write<T: Copy>(&mut self, data: &T) {
		self.try_write(data).expect("failed to copy data")
	}

	/// Try to copy data to this buffer. Returns an error if the buffer is smaller than the data.
	#[inline(always)]
	pub fn try_write<T: Copy>(&mut self, data: &T) -> Result<(), BufferTooSmall> {
		self.try_write_slice(slice::from_ref(data))
	}

	/// Copy a slice of data to this buffer.
	///
	/// # Panics
	///
	/// The buffer is too small.
	#[track_caller]
	#[inline(always)]
	pub fn write_slice<T: Copy>(&mut self, data: &[T]) {
		self.try_write_slice(data).expect("failed to copy data")
	}

	/// Try to copy a slice of data to this buffer. Returns an error if the buffer is smaller than
	/// the data.
	#[inline]
	pub fn try_write_slice<T: Copy>(&mut self, data: &[T]) -> Result<(), BufferTooSmall> {
		let size = data.len() * mem::size_of::<T>();
		if self.size < size {
			Err(BufferTooSmall)
		} else {
			// SAFETY: we have exclusive access to the memory region.
			// FIXME this may (will) copy unitialized bytes. Is this UB?
			unsafe {
				ptr::copy_nonoverlapping(data.as_ptr().cast(), self.virt.as_ptr(), size);
			}
			Ok(())
		}
	}
}

#[derive(Debug)]
pub struct BufferTooSmall;
