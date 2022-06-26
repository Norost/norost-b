use super::{Object, Ticket};
use crate::sync::Mutex;
use alloc::{boxed::Box, vec::Vec};

/// A query that returns no results.
pub struct NoneQuery;

impl Object for NoneQuery {
	fn read(&self, _: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok([].into()))
	}
}

/// Convienence wrapper to make queries from any iterator.
pub struct QueryIter<I: Iterator<Item = Vec<u8>>>(Mutex<I>);

impl<I: Iterator<Item = Vec<u8>>> QueryIter<I> {
	pub fn new(iter: I) -> Self {
		Self(iter.into())
	}
}

impl<I: Iterator<Item = Vec<u8>>> Object for QueryIter<I> {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok(self
			.0
			.lock()
			.next()
			.and_then(|b| (b.len() < length).then(move || b))
			.unwrap_or([].into())
			.into()))
	}
}
