use super::{Object, Ticket};
use crate::sync::SpinLock;
use alloc::{boxed::Box, vec::Vec};
use core::cell::Cell;

/// A query that returns no results.
pub struct NoneQuery;

impl Object for NoneQuery {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok([].into()))
	}
}

/// A query that returns a single result.
pub struct OneQuery {
	path: Cell<Box<[u8]>>,
}

impl OneQuery {
	pub fn new(path: Vec<u8>) -> Self {
		Self {
			path: Cell::new(path.into()),
		}
	}
}

impl Object for OneQuery {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok(self.path.take()))
	}
}

/// Convienence wrapper to make queries from any iterator.
pub struct QueryIter<I: Iterator<Item = Vec<u8>>>(SpinLock<I>);

impl<I: Iterator<Item = Vec<u8>>> QueryIter<I> {
	pub fn new(iter: I) -> Self {
		Self(iter.into())
	}
}

impl<I: Iterator<Item = Vec<u8>>> Object for QueryIter<I> {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok(self
			.0
			.auto_lock()
			.next()
			.and_then(|b| (b.len() < length).then(move || b))
			.unwrap_or([].into())
			.into()))
	}
}
