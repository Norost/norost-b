//! # Object tables
//!
//! An object table is a collection of objects annotated with a name and any number of tags.
//!
//! Objects can be searched/filtered with tags. Individual objects are addressed by unique
//! integer IDs.

mod streaming;

use crate::scheduler::MemoryObject;
use crate::sync::SpinLock;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

pub use streaming::StreamingTable;

/// The global list of all tables.
static TABLES: SpinLock<Vec<Weak<dyn Table>>> = SpinLock::new(Vec::new());

/// A table of objects.
pub trait Table
where
	Self: Object,
{
	/// The name of this table.
	fn name(&self) -> &str;

	/// Search for objects based on a name and/or tags.
	fn query(self: Arc<Self>, name: Option<&str>, tags: &[&str]) -> Box<dyn Query>;

	/// Get a single object based on ID.
	fn get(self: Arc<Self>, id: Id) -> Ticket;

	/// Create a new object.
	fn create(self: Arc<Self>, name: &str, tags: &[&str]) -> Ticket;

	fn take_job(&self) -> JobTask;

	fn finish_job(self: Arc<Self>, job: Job) -> Result<(), ()>;
}

/// A query into a table.
pub trait Query
where
	Self: Iterator<Item = QueryResult>,
{
}

/// A query that returns no results.
pub struct NoneQuery;

impl Query for NoneQuery {}

impl Iterator for NoneQuery {
	type Item = QueryResult;

	fn next(&mut self) -> Option<Self::Item> {
		None
	}
}

/// A query that returns a single result.
pub struct OneQuery {
	pub id: Id,
	pub tags: Option<Box<[Box<str>]>>,
}

impl Query for OneQuery {}

impl Iterator for OneQuery {
	type Item = QueryResult;

	fn next(&mut self) -> Option<Self::Item> {
		self.tags
			.take()
			.map(|tags| QueryResult { id: self.id, tags })
	}
}

/// A single query result
pub struct QueryResult {
	pub id: Id,
	pub tags: Box<[Box<str>]>,
}

/// A single object.
pub trait Object {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(&self, _offset: u64) -> Option<Box<dyn MemoryObject>> {
		None
	}

	fn read(&self, _offset: u64, _data: &mut [u8]) -> Result<Ticket, ()> {
		Err(())
	}

	fn write(&self, _offset: u64, _data: &[u8]) -> Result<Ticket, ()> {
		Err(())
	}

	fn event_listener(&self) -> Result<EventListener, Unpollable> {
		Err(Unpollable)
	}

	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		None
	}
}

/// A pollable interface to an object.
pub struct EventListener {
	shared: Arc<SpinLock<(Option<Waker>, Option<Events>)>>,
}

pub struct EventWaker {
	shared: Arc<SpinLock<(Option<Waker>, Option<Events>)>>,
}

impl EventListener {
	#[allow(dead_code)]
	fn new() -> (Self, EventWaker) {
		let shared = Arc::new(SpinLock::default());
		(
			Self {
				shared: shared.clone(),
			},
			EventWaker { shared },
		)
	}
}

impl EventWaker {
	#[allow(dead_code)]
	fn complete(self, event: Events) {
		let mut l = self.shared.lock();
		l.0.take().map(|w| w.wake());
		l.1 = Some(event);
	}
}

impl Future for EventListener {
	type Output = Events;

	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		let mut l = self.shared.lock();
		if let Some(e) = l.1.take() {
			return Poll::Ready(e);
		}
		l.0 = Some(cx.waker().clone());
		Poll::Pending
	}
}

/// A collection of events that occurred in an object
#[repr(transparent)]
pub struct Events(u32);

impl Events {
	pub const OPEN: u32 = 1 << 0;
}

bi_from!(newtype Events <=> u32);

#[derive(Debug)]
pub struct Unpollable;

/// A job submitted by a client to be fulfilled by a server (i.e. table owner).
#[derive(Default, Debug)]
pub struct Job {
	pub ty: JobType,
	pub flags: [u8; 3],
	pub job_id: JobId,
	pub operation_size: u32,
	pub object_id: Id,
	pub buffer: Box<[u8]>,
}

#[derive(Default, Debug)]
#[repr(u8)]
pub enum JobType {
	#[default]
	Open = 0,
	Read = 1,
	Write = 2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct JobId(u32);

default!(newtype JobId = u32::MAX);
bi_from!(newtype JobId <=> u32);

/// Data submitted as part of a job.
pub enum Data {
	Usize(usize),
	#[allow(dead_code)]
	Bytes(Box<[u8]>),
	Object(Arc<dyn Object>),
}

impl Data {
	pub fn into_usize(self) -> Option<usize> {
		match self {
			Self::Usize(n) => Some(n),
			_ => None,
		}
	}

	#[allow(dead_code)]
	pub fn into_bytes(self) -> Option<Box<[u8]>> {
		match self {
			Self::Bytes(n) => Some(n),
			_ => None,
		}
	}

	pub fn into_object(self) -> Option<Arc<dyn Object>> {
		match self {
			Self::Object(n) => Some(n),
			_ => None,
		}
	}
}

impl fmt::Debug for Data {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_tuple(stringify!(Data));
		match self {
			Self::Usize(n) => f.field(&n),
			Self::Bytes(d) => f.field(&d),
			Self::Object(_d) => f.field(&"<object>"),
		};
		f.finish()
	}
}

/// An error that occured during a job.
#[derive(Debug)]
pub struct Error {
	pub code: u32,
	pub message: Box<str>,
}

/// A ticket referring to a job to be completed.
#[derive(Default)]
pub struct Ticket {
	inner: Arc<SpinLock<TicketInner>>,
}

impl Ticket {
	pub fn new_complete(status: Result<Data, Error>) -> Self {
		let inner = SpinLock::new(TicketInner {
			waker: None,
			status: Some(status),
		})
		.into();
		Self { inner }
	}

	pub fn new() -> (Self, TicketWaker) {
		let inner = Arc::new(SpinLock::new(TicketInner {
			waker: None,
			status: None,
		}));
		(
			Self {
				inner: inner.clone(),
			},
			TicketWaker { inner },
		)
	}
}

pub struct TicketWaker {
	inner: Arc<SpinLock<TicketInner>>,
}

impl TicketWaker {
	fn complete(self, status: Result<Data, Error>) {
		let mut l = self.inner.lock();
		l.waker.take().map(|w| w.wake());
		l.status = Some(status);
	}
}

#[derive(Default)]
pub struct TicketInner {
	waker: Option<Waker>,
	/// The completion status of this job.
	status: Option<Result<Data, Error>>,
}

impl Future for Ticket {
	type Output = Result<Data, Error>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		let mut t = self.inner.lock();
		if let Some(s) = t.status.take() {
			return Poll::Ready(s);
		}
		t.waker = Some(cx.waker().clone());
		Poll::Pending
	}
}

/// The unique identifier of an object.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Id(pub u64);

default!(newtype Id = u64::MAX);
bi_from!(newtype Id <=> u64);

/// The unique identifier of a table.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TableId(u32);

bi_from!(newtype TableId <=> u32);

pub struct JobTask {
	shared: Arc<SpinLock<(Option<Waker>, Option<Job>)>>,
}

impl JobTask {
	pub fn new(job: Option<Job>) -> (Self, JobWaker) {
		let shared = Arc::new(SpinLock::new((None, job)));
		(
			Self {
				shared: shared.clone(),
			},
			JobWaker { shared },
		)
	}
}

impl Future for JobTask {
	type Output = Job;

	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		let mut t = self.shared.lock();
		if let Some(s) = t.1.take() {
			return Poll::Ready(s);
		}
		t.0 = Some(cx.waker().clone());
		Poll::Pending
	}
}

pub struct JobWaker {
	shared: Arc<SpinLock<(Option<Waker>, Option<Job>)>>,
}

impl JobWaker {
	fn complete(self, job: Job) {
		let mut l = self.shared.lock();
		l.0.take().map(|w| w.wake());
		l.1 = Some(job);
	}
}

/// Get a list of all tables with their respective name and ID.
#[allow(dead_code)]
pub fn tables() -> Vec<(Box<str>, TableId)> {
	TABLES
		.lock()
		.iter()
		.enumerate()
		.filter_map(|(i, t)| t.upgrade().map(|t| (t.name().into(), TableId(i as u32))))
		.collect()
}

/// Get the ID of the table with the given name.
#[allow(dead_code)]
pub fn find_table(name: &str) -> Option<TableId> {
	TABLES
		.lock()
		.iter()
		.position(|e| e.upgrade().map_or(false, |e| e.name() == name))
		.map(|i| TableId(i as u32))
}

/// Perform a query on the given table if it exists.
pub fn query(
	table_id: TableId,
	name: Option<&str>,
	tags: &[&str],
) -> Result<Box<dyn Query>, QueryError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.map(|tbl| tbl.query(name, tags))
		.ok_or(QueryError::InvalidTableId)
}

/// Get an object from a table.
pub fn get(table_id: TableId, id: Id) -> Result<Ticket, GetError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.map(|tbl| tbl.get(id))
		.ok_or(GetError::InvalidTableId)
}

/// Create a new object in a table.
#[allow(dead_code)]
pub fn create(table_id: TableId, name: &str, tags: &[&str]) -> Result<Ticket, CreateError> {
	TABLES
		.lock()
		.get(usize::try_from(table_id.0).unwrap())
		.and_then(Weak::upgrade)
		.ok_or(CreateError::InvalidTableId)
		.map(|tbl| tbl.create(name, tags))
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
	#[allow(dead_code)]
	InvalidTableId,
}

#[derive(Debug)]
pub struct CreateObjectError {
	pub message: Box<str>,
}
