use super::{Object, Ticket};
use crate::sync::Mutex;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::iter::Peekable;

/// A query that returns no results.
pub struct NoneQuery;

impl Object for NoneQuery {
	fn read(self: Arc<Self>, _: usize, _: bool) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok([].into()))
	}
}

/// Convienence wrapper to make queries from any iterator.
pub struct QueryIter<I: Iterator<Item = Vec<u8>>>(Mutex<Peekable<I>>);

impl<I: Iterator<Item = Vec<u8>>> QueryIter<I> {
	#[inline]
	pub fn new<C: IntoIterator<IntoIter = I>>(iter: C) -> Self {
		Self(iter.into_iter().peekable().into())
	}
}

impl<I: Iterator<Item = Vec<u8>>> Object for QueryIter<I> {
	fn read(self: Arc<Self>, length: usize, peek: bool) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok({
			let mut it = self.0.lock();
			if peek { it.peek().cloned() } else { it.next() }
				.and_then(|b| (b.len() < length).then(move || b))
				.unwrap_or([].into())
				.into()
		}))
	}
}
