use crate::Slice;
use core::{
	intrinsics, marker::PhantomData, mem::MaybeUninit, ptr::NonNull, sync::atomic::AtomicU32,
};

#[derive(Clone, Copy)]
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

	unsafe fn from_offset(base: NonNull<u8>, offset: u32, size: u32) -> Self {
		Self {
			base: NonNull::new_unchecked(base.as_ptr().add(offset as usize * size as usize)),
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
		unsafe { self.copy_from_raw(offset, buf.as_ptr(), buf.len()) }
	}

	#[inline]
	pub unsafe fn copy_from_raw(&self, offset: usize, dst: *const u8, count: usize) {
		self.copy_from_raw_untrusted(offset, dst, count)
	}

	#[inline]
	pub unsafe fn copy_from_raw_untrusted(&self, offset: usize, src: *const u8, count: usize) {
		assert!(offset + count <= self.size as usize);
		intrinsics::volatile_copy_nonoverlapping_memory(self.base.as_ptr().add(offset), src, count)
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
	pub fn copy_to_untrusted(&self, offset: usize, buf: &mut [u8]) {
		unsafe { self.copy_to_raw_untrusted(offset, buf.as_mut_ptr(), buf.len()) }
	}

	#[inline]
	pub fn copy_to_untrusted_uninit(&self, offset: usize, buf: &mut [u8]) {
		unsafe { self.copy_to_raw_untrusted(offset, buf.as_mut_ptr(), buf.len()) }
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
	pub fn get<'a>(&'a self, slice: Slice) -> Data<'a> {
		let max = slice.offset as usize * self.block_size as usize;
		assert!(max < self.total_size, "out of bounds");
		Data {
			buffers: self,
			offset: slice.offset,
			len: slice.length.try_into().unwrap(),
		}
	}

	#[inline]
	pub fn alloc_empty<'a>(&'a self) -> Data<'a> {
		Data {
			buffers: self,
			offset: 0,
			len: 0,
		}
	}

	#[inline]
	pub fn alloc<'a>(&'a self, head: &AtomicU32, size: usize) -> Option<Data<'a>> {
		if size == 0 {
			return Some(self.alloc_empty());
		}

		if size <= self.block_size as usize {
			return Some(Data {
				buffers: self,
				offset: unsafe { crate::stack::pop(head, self.base, self.block_size)? },
				len: size.try_into().unwrap(),
			});
		}

		let mut l = size;
		let offset @ mut base = unsafe { crate::stack::pop(head, self.base, self.block_size)? };
		'l: loop {
			let b = self.get_buf(base);
			for i in 0..self.block_size / 4 {
				if l == 0 {
					break 'l;
				}
				base = unsafe { crate::stack::pop(head, self.base, self.block_size)? };
				b.copy_from((i * 4) as usize, &base.to_le_bytes());
				l = l.saturating_sub(self.block_size as _);
			}
			if l == 0 {
				break 'l;
			}
			l += self.block_size as usize;
		}
		Some(Data {
			buffers: self,
			offset,
			len: size.try_into().unwrap(),
		})
	}

	#[inline]
	pub fn dealloc(&self, head: &AtomicU32, buf: u32) {
		assert!(
			(buf as usize * self.block_size as usize) < self.total_size,
			"buffer index out of range"
		);
		unsafe { crate::stack::push(head, self.base, buf, self.block_size) }
	}

	fn get_buf(&self, offset: u32) -> Buffer<'_> {
		unsafe { Buffer::from_offset(self.base, offset, self.block_size) }
	}
}

pub struct Data<'a> {
	buffers: &'a Buffers,
	offset: u32,
	len: u32,
}

impl<'a> Data<'a> {
	#[inline]
	pub fn copy_from(&self, offset: usize, buf: &[u8]) {
		unsafe { self.copy_from_raw(offset, buf.as_ptr(), buf.len()) }
	}

	#[inline]
	pub unsafe fn copy_from_raw(&self, offset: usize, src: *const u8, count: usize) {
		self.copy_from_raw_untrusted(offset, src, count)
	}

	#[inline]
	pub unsafe fn copy_from_raw_untrusted(
		&self,
		mut offset: usize,
		mut src: *const u8,
		mut count: usize,
	) {
		assert!(offset + count <= self.len as usize, "out of bounds");
		let bs = self.buffers.block_size as usize;
		let skip = (offset / bs).try_into().unwrap();
		offset %= bs;
		for b in self.blocks().skip(skip) {
			let c = count.min(bs - offset);
			b.1.copy_from_raw_untrusted(offset, src, c);
			src = src.add(c);
			offset = 0;
			count -= c;
			if count == 0 {
				break;
			}
		}
	}

	#[cfg_attr(debug_assertions, track_caller)]
	#[inline]
	pub fn copy_to(&self, offset: usize, buf: &mut [u8]) {
		unsafe { self.copy_to_raw(offset, buf.as_mut_ptr(), buf.len()) }
	}

	#[inline]
	pub fn copy_to_uninit(&self, offset: usize, buf: &mut [MaybeUninit<u8>]) {
		unsafe { self.copy_to_raw(offset, buf.as_mut_ptr().cast(), buf.len()) }
	}

	#[inline]
	pub fn copy_to_untrusted(&self, offset: usize, buf: &mut [u8]) {
		unsafe { self.copy_to_raw_untrusted(offset, buf.as_mut_ptr(), buf.len()) }
	}

	#[inline]
	pub fn copy_to_untrusted_uninit(&self, offset: usize, buf: &mut [MaybeUninit<u8>]) {
		unsafe { self.copy_to_raw_untrusted(offset, buf.as_mut_ptr().cast(), buf.len()) }
	}

	#[cfg_attr(debug_assertions, track_caller)]
	#[inline]
	pub unsafe fn copy_to_raw(&self, offset: usize, dst: *mut u8, count: usize) {
		self.copy_to_raw_untrusted(offset, dst, count)
	}

	#[cfg_attr(debug_assertions, track_caller)]
	#[inline]
	pub unsafe fn copy_to_raw_untrusted(
		&self,
		mut offset: usize,
		mut dst: *mut u8,
		mut count: usize,
	) {
		assert!(offset + count <= self.len as usize, "naughty proces");
		let bs = self.buffers.block_size as usize;
		let skip = (offset / bs).try_into().unwrap();
		offset %= bs;
		for b in self.blocks().skip(skip) {
			let c = count.min(bs - offset);
			b.1.copy_to_raw_untrusted(offset, dst, c);
			dst = dst.add(c);
			offset = 0;
			count -= c;
			if count == 0 {
				break;
			}
		}
	}

	#[inline]
	pub fn offset(&self) -> usize {
		self.offset.try_into().unwrap()
	}

	#[inline]
	pub fn len(&self) -> usize {
		self.len.try_into().unwrap()
	}

	// FIXME make Drop trait work in match
	pub fn manual_drop(self, head: &'a AtomicU32) {
		if self.len == 0 {
			return;
		}
		let (mut l, mut o, mut n) = (self.len, self.offset, [0; 4]);
		loop {
			let bo = o;
			let b = self.buffers.get_buf(bo);
			let to = self.buffers.block_size / 4;
			for i in 0..to {
				if l <= self.buffers.block_size {
					if i > 0 {
						b.copy_to(i as usize * 4, &mut n);
						o = u32::from_le_bytes(n);
						self.buffers.dealloc(head, o);
					}
					self.buffers.dealloc(head, bo);
					return;
				} else {
					b.copy_to(i as usize * 4, &mut n);
					o = u32::from_le_bytes(n);
					l -= self.buffers.block_size;
					if i != to - 1 {
						self.buffers.dealloc(head, o);
					}
				}
			}
			// Account for scatter-gather array block
			self.buffers.dealloc(head, bo);
			l += self.buffers.block_size;
		}
	}

	/// ```
	/// D0
	///
	///  L0 -------> Dm
	/// /  \
	/// D0 D1 ..
	///
	/// ...
	///
	///  L0 -------> L1 -------> .. -------> LN -------> Dn
	/// /  \        /  \                    /  \
	/// D0 D1 ..   Dm  Dm+1 ..             ..  Dn-1
	/// ```
	pub fn blocks(&self) -> DataIter<'_, 'a> {
		DataIter {
			buffers: self.buffers,
			offset: self.offset,
			count: self.len.div_ceil(self.buffers.block_size),
			// Even if len is 0 it's fine to take any buffer as long as we never actually use it
			cur_buf: self.buffers.get_buf(self.offset),
			cur_i: 0,
			_marker: PhantomData,
		}
	}
}

pub struct DataIter<'a, 'b: 'a> {
	buffers: &'b Buffers,
	offset: u32,
	count: u32,
	cur_buf: Buffer<'b>,
	cur_i: u32,
	_marker: PhantomData<&'a Data<'b>>,
}

// TODO implement efficient skip()
//
// While it should optimize fine for now, it won't if (when) we reuse this code for untrusted
// processes and need to use copy_to_untrusted.
impl<'a, 'b: 'a> Iterator for DataIter<'a, 'b> {
	type Item = (u32, Buffer<'b>);

	fn next(&mut self) -> Option<Self::Item> {
		if self.count == 0 {
			return None;
		}

		// Looping makes it easier to deal with scatter list blocks
		loop {
			let to = self.buffers.block_size / 4;
			if self.count == 1 {
				// Return the last block
				// Check if the current block is the one to return, otherwise return an entry
				//
				//   L----->D               L------>L
				//  / \            vs      / \     / \
				// D   D                  D   D   D   D
				if self.cur_i > 0 {
					debug_assert!(self.cur_i < to);
					let mut n = [0; 4];
					self.cur_buf.copy_to(self.cur_i as usize * 4, &mut n);
					self.offset = u32::from_le_bytes(n);
					self.cur_buf = self.buffers.get_buf(self.offset);
				}
				self.count -= 1;
				return Some((self.offset, self.cur_buf));
			} else {
				// We're not at the last block yet.
				debug_assert!(self.cur_i < to);
				let mut n = [0; 4];
				self.cur_buf.copy_to(self.cur_i as usize * 4, &mut n);
				self.offset = u32::from_le_bytes(n);
				let b = self.buffers.get_buf(self.offset);
				// If this block is
				// - Not the last block
				// - The last entry in the current block
				// It is part of the chain and must not be returned as a data block
				if self.cur_i == to - 1 {
					self.cur_buf = b;
					self.cur_i = 0;
				} else {
					self.cur_i += 1;
					self.count -= 1;
					return Some((self.offset, b));
				}
			};
		}
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.len(), Some(self.len()))
	}
}

impl<'a, 'b: 'a> ExactSizeIterator for DataIter<'a, 'b> {
	fn len(&self) -> usize {
		self.count.try_into().unwrap()
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
