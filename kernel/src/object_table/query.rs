use super::Ticket;
use alloc::boxed::Box;

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
