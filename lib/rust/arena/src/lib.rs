//! # Typed arena with optional generational identifiers.

#![no_std]
#![feature(const_default_impls, const_trait_impl)]

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use {
	alloc::vec,
	core::{
		fmt, iter, mem,
		ops::{Index, IndexMut},
		slice,
	},
};

/// A typed arena. A generation type can be specified which is used to prevent the ABA problem.
pub struct Arena<V, G: Generation> {
	storage: vec::Vec<Entry<V, G>>,
	free: usize,
	generation: G,
	count: usize,
}

impl<V, G: Generation> fmt::Debug for Arena<V, G>
where
	V: fmt::Debug,
	G: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_map().entries(self.iter()).finish()
	}
}

enum Entry<V, G: Generation> {
	Free { next: usize },
	Occupied { value: V, generation: G },
}

#[derive(Clone, Copy)]
pub struct Handle<G: Generation> {
	index: usize,
	generation: G,
}

impl<G: Generation> fmt::Debug for Handle<G>
where
	G: fmt::Debug,
{
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if mem::size_of::<G>() == 0 {
			self.index.fmt(f)
		} else {
			self.index.fmt(f)?;
			f.write_str("@")?;
			self.generation.fmt(f)
		}
	}
}

impl<G: Generation> Handle<G> {
	pub fn into_raw(self) -> (usize, G) {
		(self.index, self.generation)
	}

	pub fn from_raw(index: usize, generation: G) -> Self {
		Self { index, generation }
	}
}

pub trait Generation: Copy + Eq {
	fn increment(&mut self);
}

impl Generation for () {
	fn increment(&mut self) {}
}

macro_rules! impl_int {
	($ty:ty) => {
		impl Generation for $ty {
			fn increment(&mut self) {
				*self = self.wrapping_add(1);
			}
		}
	};
}

impl_int!(u8);
impl_int!(u16);
impl_int!(u32);
impl_int!(u64);
impl_int!(u128);
impl_int!(i8);
impl_int!(i16);
impl_int!(i32);
impl_int!(i64);
impl_int!(i128);

impl<V, G: Generation + Default> Arena<V, G> {
	pub const fn new() -> Self {
		Default::default()
	}
}

impl<V, G: Generation> Arena<V, G> {
	pub fn insert(&mut self, value: V) -> Handle<G> {
		self.insert_with(|_| value)
	}

	pub fn insert_with(&mut self, f: impl FnOnce(Handle<G>) -> V) -> Handle<G> {
		let generation = self.generation.clone();
		self.generation.increment();
		if self.free != usize::MAX {
			let handle = Handle { index: self.free, generation };
			let entry = Entry::Occupied { value: f(handle), generation };
			match mem::replace(&mut self.storage[self.free], entry) {
				Entry::Free { next } => self.free = next,
				Entry::Occupied { .. } => unreachable!(),
			}
			self.count += 1;
			handle
		} else {
			let handle = Handle { index: self.storage.len(), generation };
			self.storage
				.push(Entry::Occupied { value: f(handle), generation });
			self.count += 1;
			handle
		}
	}

	pub fn remove(&mut self, handle: Handle<G>) -> Option<V> {
		match self.storage.get(handle.index)? {
			Entry::Free { .. } => None,
			Entry::Occupied { generation, .. } => {
				if generation != &handle.generation {
					return None;
				}
				let entry = Entry::Free { next: self.free };
				match mem::replace(&mut self.storage[handle.index], entry) {
					Entry::Occupied { value, .. } => {
						self.free = handle.index;
						self.count -= 1;
						Some(value)
					}
					Entry::Free { .. } => unreachable!(),
				}
			}
		}
	}

	pub fn iter(&self) -> Iter<'_, V, G> {
		Iter { inner: self.storage.iter().enumerate() }
	}

	pub fn iter_mut(&mut self) -> IterMut<'_, V, G> {
		IterMut { inner: self.storage.iter_mut().enumerate() }
	}

	pub fn drain(&mut self) -> Drain<'_, V, G> {
		self.free = usize::MAX;
		self.count = 0;
		Drain { inner: self.storage.drain(..).enumerate() }
	}

	pub fn get(&self, handle: Handle<G>) -> Option<&V> {
		match self.storage.get(handle.index)? {
			Entry::Occupied { value, generation } if generation == &handle.generation => {
				Some(value)
			}
			_ => None,
		}
	}

	pub fn get_mut(&mut self, handle: Handle<G>) -> Option<&mut V> {
		match self.storage.get_mut(handle.index)? {
			Entry::Occupied { value, generation } if generation == &handle.generation => {
				Some(value)
			}
			_ => None,
		}
	}

	#[inline(always)]
	pub fn len(&self) -> usize {
		self.count
	}

	#[inline(always)]
	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	#[inline]
	pub fn clear(&mut self) {
		self.storage.clear();
		self.count = 0;
		self.free = usize::MAX;
	}
}

impl<V, G: Generation> Index<Handle<G>> for Arena<V, G> {
	type Output = V;

	fn index(&self, handle: Handle<G>) -> &Self::Output {
		self.get(handle).expect("no item with handle")
	}
}

impl<V, G: Generation> IndexMut<Handle<G>> for Arena<V, G> {
	fn index_mut(&mut self, handle: Handle<G>) -> &mut Self::Output {
		self.get_mut(handle).expect("no item with handle")
	}
}

impl<V, G: Generation + ~const Default> const Default for Arena<V, G> {
	fn default() -> Self {
		Self {
			storage: Default::default(),
			free: usize::MAX,
			generation: Default::default(),
			count: 0,
		}
	}
}

macro_rules! iter {
	($name:ident, $it:ident, $ty:ty) => {
		pub struct $name<'a, V, G: Generation> {
			inner: iter::Enumerate<$it::$name<'a, Entry<V, G>>>,
		}

		impl<'a, V, G: Generation> Iterator for $name<'a, V, G> {
			type Item = (Handle<G>, $ty);

			fn next(&mut self) -> Option<Self::Item> {
				while let Some((index, value)) = self.inner.next() {
					match value {
						Entry::Occupied { value, generation } => {
							let generation = generation.clone();
							return Some((Handle { index, generation }, value));
						}
						Entry::Free { .. } => {}
					}
				}
				None
			}
		}
	};
}

iter!(Iter, slice, &'a V);
iter!(IterMut, slice, &'a mut V);
iter!(Drain, vec, V);
