use super::{JobId, JobResult, JobTask, Object, Query, Ticket};
use crate::sync::SpinLock;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::time::Duration;

/// A table of objects.
pub trait Table {
	/// The name of this table.
	fn name(&self) -> &str;

	/// Search for objects based on a name and/or tags.
	fn query(self: Arc<Self>, path: &[u8]) -> Ticket<Box<dyn Query>>;

	/// Open a single object based on path.
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>>;

	/// Create a new object with the given path.
	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>>;

	fn take_job(self: Arc<Self>, _timeout: Duration) -> JobTask {
		unimplemented!()
	}

	fn finish_job(self: Arc<Self>, _job: JobResult, _job_id: JobId) -> Result<(), ()> {
		unimplemented!()
	}

	fn cancel_job(self: Arc<Self>, _job_id: JobId) {
		unimplemented!()
	}
}

/// The global list of all tables.
static TABLES: SpinLock<Vec<Weak<dyn Table>>> = SpinLock::new(Vec::new());

#[derive(Clone, Copy)]
pub struct Cancelled;

/// The unique identifier of a table.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TableId(pub u32);

bi_from!(newtype TableId <=> u32);

/// Perform a query on the given table if it exists.
pub fn query(table_id: TableId, path: &[u8]) -> Result<Ticket<Box<dyn Query>>, QueryError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.map(|tbl| tbl.query(path))
		.ok_or(QueryError::InvalidTableId)
}

/// Open an object from a table.
pub fn open(table_id: TableId, path: &[u8]) -> Result<Ticket<Arc<dyn Object>>, GetError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.map(|tbl| tbl.open(path))
		.ok_or(GetError::InvalidTableId)
}

/// Create a new object in a table.
pub fn create(table_id: TableId, path: &[u8]) -> Result<Ticket<Arc<dyn Object>>, CreateError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.ok_or(CreateError::InvalidTableId)
		.map(|tbl| tbl.create(path))
}

/// Add a new table.
#[optimize(size)]
pub fn add_table(table: Weak<dyn Table>) -> TableId {
	let mut tbl = TABLES.lock();
	tbl.push(table);
	TableId((tbl.len() - 1) as u32)
}

/// Return the name and ID of the table after another table, or the first table if `id` is `None`.
pub fn next_table(id: Option<TableId>) -> Option<(Box<str>, TableId)> {
	let tbl = TABLES.lock();
	let (id, tbl) = match id {
		None => tbl
			.iter()
			.enumerate()
			.find_map(|(i, t)| t.upgrade().map(|t| (i, t))),
		Some(id) => tbl.iter().enumerate().find_map(|(i, t)| {
			t.upgrade()
				.and_then(|t| (i > id.0 as usize).then(|| (i, t)))
		}),
	}?;
	Some((tbl.name().into(), TableId(id as u32)))
}

#[derive(Debug)]
pub enum QueryError {
	InvalidTableId,
}

#[derive(Debug)]
pub enum GetError {
	InvalidTableId,
}

#[derive(Debug)]
pub enum CreateError {
	InvalidTableId,
}

#[derive(Debug)]
pub struct CreateObjectError {
	pub message: Box<str>,
}
