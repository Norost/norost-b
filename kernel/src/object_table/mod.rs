//! # Object tables
//!
//! An object table is a collection of objects annotated with a name and any number of tags.
//!
//! Objects can be searched/filtered by a combination of name and/or tags. Individual objects are
//! addressed by unique integer IDs.

use crate::scheduler::MemoryObject;
use crate::sync::SpinLock;
use alloc::{boxed::Box, vec::Vec};
use core::ops::{Deref, DerefMut};

/// The global list of all tables.
static TABLES: SpinLock<Vec<Option<Box<dyn Table>>>> = SpinLock::new(Vec::new());

/// A table of objects.
pub trait Table {
	/// The name of this table.
	fn name(&self) -> &str;

	/// Search for objects based on a name and/or tags.
	fn query(&self, name: Option<&str>, tags: &[&str]) -> Box<dyn Query>;

	/// Get a single object based on ID.
	fn get(&self, id: Id) -> Option<Object>;

	/// Create a new object.
	fn create(&self, name: &str, tags: &[&str]) -> Result<Object, CreateObjectError>;
}

/// A query into a table.
pub trait Query
where
	Self: Iterator<Item = Object>,
{
}

/// A single object.
pub struct Object {
	/// The ID of this object.
	pub id: Id,
	/// The name of the object.
	pub name: Box<str>,
	/// Tags associated with the object.
	pub tags: Box<[Box<str>]>,
	/// The interface to interact with this object.
	pub interface: Box<dyn Interface>,
}

impl Deref for Object {
	type Target = dyn Interface;

	fn deref(&self) -> &Self::Target {
		&*self.interface
	}
}

impl DerefMut for Object {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut *self.interface
	}
}

/// An interface to an object.
pub trait Interface {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(&self) -> Option<Box<dyn MemoryObject>>;
}

/// The unique identifier of an object.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Id(u64);

impl From<Id> for u64 {
	fn from(i: Id) -> Self {
		i.0
	}
}

impl From<u64> for Id {
	fn from(n: u64) -> Id {
		Self(n)
	}
}

/// The unique identifier of a table.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TableId(u32);

/// Get a list of all tables with their respective name and ID.
pub fn tables() -> Vec<(Box<str>, TableId)> {
	TABLES
		.lock()
		.iter()
		.enumerate()
		.filter_map(|(i, t)| t.as_ref().map(|t| (t.name().into(), TableId(i as u32))))
		.collect()
}

/// Get the ID of the table with the given name.
pub fn find_table(name: &str) -> Option<TableId> {
	TABLES
		.lock()
		.iter()
		.position(|e| e.as_ref().map_or(false, |e| e.name() == name))
		.map(|i| TableId(i as u32))
}

/// Perform a query on the given table if it exists.
pub fn query(table_id: TableId, name: Option<&str>, tags: &[&str]) -> Result<Box<dyn Query>, QueryError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Option::as_ref)
		.map(|tbl| tbl.query(name, tags))
		.ok_or(QueryError::InvalidTableId)
}

/// Get an object from a table.
pub fn get(table_id: TableId, id: Id) -> Result<Option<Object>, GetError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Option::as_ref)
		.map(|tbl| tbl.get(id))
		.ok_or(GetError::InvalidTableId)
}

/// Create a new object in a table.
pub fn create(table_id: TableId, name: &str, tags: &[&str]) -> Result<Object, CreateError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Option::as_ref)
		.ok_or(CreateError::InvalidTableId)
		.and_then(|tbl| tbl.create(name, tags).map_err(CreateError::CreateObjectError))
}

/// Add a new table.
#[optimize(size)]
pub fn add_table(table: impl Table + 'static) -> TableId {
	// Inner function to reduce code size
	#[optimize(size)]
	fn add(table: Box<dyn Table>) -> TableId {
		let mut tbl = TABLES.lock();
		let id = TableId(tbl.len() as u32);
		tbl.push(Some(table));
		id
	}
	add(Box::new(table))
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
	CreateObjectError(CreateObjectError),
}

#[derive(Debug)]
pub struct CreateObjectError {
	pub message: Box<str>,
}
