use crate::Slice;
use core::{
	intrinsics,
	marker::PhantomData,
	mem::{self, MaybeUninit},
	ptr::NonNull,
	sync::atomic::AtomicU32,
};

pub struct Buffer<'a> {
	base: NonNull<u8>,
	size: u32,
	_marker: PhantomData<&'a u8>,
}

impl Buffer<'_> {
	pub const EMPTY: Buffer<'static> = Buffer {
		base: NonNull::dangling(),
		size: 0,
		_marker: PhantomData,
	};

	#[inline(always)]
	unsafe fn new(base: NonNull<u8>, size: u32) -> Self {
		Self {
			base,
			size,
			_marker: PhantomData,
		}
	}

	#[inline(always)]
	pub fn as_ptr(&self) -> *const u8 {
		self.base.as_ptr()
	}

	#[inline(always)]
	pub fn as_mut_ptr(&self) -> *const u8 {
		self.base.as_ptr()
	}

	#[inline]
	pub fn copy_from(&self, offset: usize, buf: &[u8]) {
		assert!(offset + buf.len() <= self.size as usize);
		unsafe {
			intrinsics::volatile_copy_nonoverlapping_memory(
				self.base.as_ptr().add(offset),
				buf.as_ptr(),
				buf.len(),
			)
		}
	}

	#[inline]
	pub fn copy_to(&self, offset: usize, buf: &mut [u8]) {
		unsafe { self.copy_to_raw(offset, buf.as_mut_ptr(), buf.len()) }
	}

	#[inline]
	pub fn copy_to_uninit(&self, offset: usize, buf: &mut [MaybeUninit<u8>]) {
		unsafe { self.copy_to_raw(offset, buf.as_mut_ptr().cast(), buf.len()) }
	}

	#[inline]
	pub unsafe fn copy_to_raw(&self, offset: usize, dst: *mut u8, count: usize) {
		self.copy_to_raw_untrusted(offset, dst, count)
	}

	#[inline]
	pub unsafe fn copy_to_raw_untrusted(&self, offset: usize, dst: *mut u8, count: usize) {
		assert!(offset + count <= self.size as usize);
		intrinsics::volatile_copy_nonoverlapping_memory(dst, self.base.as_ptr().add(offset), count)
	}

	#[inline]
	pub fn len(&self) -> usize {
		self.size.try_into().unwrap()
	}

	#[inline]
	pub fn array_chunks<const N: usize>(&self) -> ArrayChunks<'_, N> {
		ArrayChunks {
			base: self.base,
			len: self.size.try_into().unwrap(),
			_marker: PhantomData,
		}
	}
}

pub struct Buffers {
	base: NonNull<u8>,
	total_size: usize,
	block_size: u32,
}

pub enum Data<'a> {
	Single(Buffer<'a>),
}

impl Buffers {
	#[inline(always)]
	pub unsafe fn new(base: NonNull<u8>, total_size: usize, block_size: u32) -> Self {
		debug_assert_eq!(
			block_size.count_ones(),
			1,
			"block size is not a power of two"
		);
		Self {
			base,
			total_size,
			block_size,
		}
	}

	#[inline]
	pub fn get<'a>(&'a self, slice: Slice) -> impl Iterator<Item = Buffer<'a>> {
		assert!(slice.length <= self.block_size, "TODO");
		let max = slice.offset as usize * self.block_size as usize + slice.length as usize;
		assert!(max <= self.total_size, "out of bounds");
		Resolver {
			base: self.base,
			len: slice.length,
			offset: slice.offset,
			block_size: self.block_size,
			_marker: PhantomData,
		}
	}

	#[inline]
	pub fn alloc<'a>(&'a self, head: &AtomicU32, size: usize) -> Option<(Data<'a>, u32)> {
		assert!(size <= self.block_size as usize, "TODO");
		unsafe {
			let offset = crate::stack::pop(head, self.base, self.block_size)?;
			let p = self
				.base
				.as_ptr()
				.add(offset as usize * self.block_size as usize);
			Some((
				Data::Single(Buffer::new(NonNull::new_unchecked(p), size as _)),
				offset,
			))
		}
	}

	#[inline]
	pub fn dealloc(&self, head: &AtomicU32, buf: u32) {
		assert!(
			(buf as usize * self.block_size as usize) < self.total_size,
			"buffer index out of range"
		);
		unsafe { crate::stack::push(head, self.base, buf, self.block_size) }
	}
}

struct Resolver<'a> {
	base: NonNull<u8>,
	len: u32,
	offset: u32,
	block_size: u32,
	_marker: PhantomData<&'a Buffers>,
}

impl<'a> Resolver<'a> {}

impl<'a> Iterator for Resolver<'a> {
	type Item = Buffer<'a>;

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		if self.len == 0 {
			None
		} else if let Some(l) = self.len.checked_sub(self.block_size) {
			self.len = l;
			todo!()
		} else {
			unsafe {
				Some(Buffer::new(
					NonNull::new_unchecked(
						self.base
							.as_ptr()
							.add(self.offset as usize * self.block_size as usize),
					),
					mem::take(&mut self.len),
				))
			}
		}
	}
}

pub struct ArrayChunks<'a, const N: usize> {
	base: NonNull<u8>,
	len: usize,
	_marker: PhantomData<Buffer<'a>>,
}

impl<'a, const N: usize> ArrayChunks<'a, N> {
	#[inline(always)]
	pub fn remainder(&mut self) -> FixedSlice<u8, N> {
		assert!(self.len < N, "iterator has not finished");
		let mut a = MaybeUninit::uninit_array::<N>();
		unsafe {
			let p = self.base.as_ptr();
			intrinsics::volatile_copy_nonoverlapping_memory(a.as_mut_ptr().cast(), p, N)
		}
		FixedSlice {
			storage: a,
			len: self.len,
		}
	}
}

impl<'a, const N: usize> Iterator for ArrayChunks<'a, N> {
	type Item = [u8; N];

	#[inline]
	fn next(&mut self) -> Option<Self::Item> {
		self.len.checked_sub(N).map(|l| {
			self.len = l;
			let mut a = [0; N];
			unsafe {
				let p = self.base.as_ptr();
				intrinsics::volatile_copy_nonoverlapping_memory(a.as_mut_ptr(), p, N);
				self.base = NonNull::new_unchecked(p.add(N));
			}
			a
		})
	}
}

pub struct FixedSlice<T, const N: usize> {
	storage: [MaybeUninit<T>; N],
	len: usize,
}

impl<T, const N: usize> AsRef<[T]> for FixedSlice<T, N> {
	fn as_ref(&self) -> &[T] {
		// SAFETY: all elements up to len are initialized.
		unsafe { MaybeUninit::slice_assume_init_ref(&self.storage[..self.len]) }
	}
}
