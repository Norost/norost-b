use {
	super::{Object, Ticket},
	crate::sync::Mutex,
	alloc::{boxed::Box, sync::Arc, vec::Vec},
};

/// A query that returns no results.
pub struct NoneQuery;

impl Object for NoneQuery {
	fn read(self: Arc<Self>, _: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok([].into()))
	}
}

/// Convienence wrapper to make queries from any iterator.
pub struct QueryIter<I: Iterator<Item = Vec<u8>>>(Mutex<I>);

impl<I: Iterator<Item = Vec<u8>>> QueryIter<I> {
	#[inline]
	pub fn new<C: IntoIterator<IntoIter = I>>(iter: C) -> Self {
		Self(iter.into_iter().into())
	}
}

impl<I: Iterator<Item = Vec<u8>>> Object for QueryIter<I> {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok({
			let mut it = self.0.lock();
			it.next()
				.and_then(|b| (b.len() < length).then(move || b))
				.unwrap_or([].into())
				.into()
		}))
	}
}
