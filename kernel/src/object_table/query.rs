use super::Ticket;
use alloc::{boxed::Box, vec::Vec};

/// A query into a table.
pub trait Query
where
	Self: Iterator<Item = Ticket<QueryResult>>,
{
}

/// A query that returns no results.
pub struct NoneQuery;

impl Query for NoneQuery {}

impl Iterator for NoneQuery {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		None
	}
}

/// A query that returns a single result.
pub struct OneQuery {
	pub path: Option<Box<[u8]>>,
}

impl Query for OneQuery {}

impl Iterator for OneQuery {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		self.path
			.take()
			.map(|path| Ticket::new_complete(Ok(QueryResult { path })))
	}
}

/// A single query result
pub struct QueryResult {
	pub path: Box<[u8]>,
}

/// Convienence wrapper to make queries from any iterator.
pub struct QueryIter<I: Iterator<Item = Vec<u8>>>(I);

impl<I: Iterator<Item = Vec<u8>>> QueryIter<I> {
	pub fn new(iter: I) -> Self {
		Self(iter)
	}
}

impl<I: Iterator<Item = Vec<u8>>> Iterator for QueryIter<I> {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		self.0
			.next()
			.map(|path| Ticket::new_complete(Ok(QueryResult { path: path.into() })))
	}
}

impl<I: Iterator<Item = Vec<u8>>> Query for QueryIter<I> {}
