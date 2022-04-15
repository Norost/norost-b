use crate::Handle;
use core::ops::{Index, IndexMut};

/// A typed arena that takes a [`Handle`] as key.
///
/// This is commonly used with kernel tables, as those need a unique [`Handle`] per resource.
#[derive(Default)]
pub struct Arena<T> {
	inner: ::arena::Arena<T, ()>,
}

impl<T> Arena<T> {
	pub fn new() -> Self {
		Self {
			inner: ::arena::Arena::new(),
		}
	}

	pub fn insert(&mut self, value: T) -> Handle {
		Self::convert_from_handle(self.inner.insert(value)).expect("index out of bounds")
	}

	pub fn remove(&mut self, handle: Handle) -> Option<T> {
		self.inner.remove(Self::convert_to_handle(handle)?)
	}

	pub fn get(&self, handle: Handle) -> Option<&T> {
		self.inner.get(Self::convert_to_handle(handle)?)
	}

	pub fn get_mut(&mut self, handle: Handle) -> Option<&mut T> {
		self.inner.get_mut(Self::convert_to_handle(handle)?)
	}

	fn convert_to_handle(handle: Handle) -> Option<::arena::Handle<()>> {
		handle
			.try_into()
			.ok()
			.map(|h| ::arena::Handle::from_raw(h, ()))
	}

	fn convert_from_handle(handle: ::arena::Handle<()>) -> Option<Handle> {
		handle.into_raw().0.try_into().ok()
	}
}

impl<T> Index<Handle> for Arena<T> {
	type Output = T;

	fn index(&self, handle: Handle) -> &Self::Output {
		&self.inner[::arena::Handle::from_raw(handle.try_into().unwrap(), ())]
	}
}

impl<T> IndexMut<Handle> for Arena<T> {
	fn index_mut(&mut self, handle: Handle) -> &mut Self::Output {
		&mut self.inner[::arena::Handle::from_raw(handle.try_into().unwrap(), ())]
	}
}
