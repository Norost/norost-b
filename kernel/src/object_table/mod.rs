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
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use core::time::Duration;

pub use norostb_kernel::{
	io::{JobId, SeekFrom},
	syscall::Handle,
};
pub use streaming::StreamingTable;

/// The global list of all tables.
static TABLES: SpinLock<Vec<Weak<dyn Table>>> = SpinLock::new(Vec::new());

#[derive(Clone, Copy)]
pub struct Timeout;

#[derive(Clone, Copy)]
pub struct Cancelled;

/// A table of objects.
pub trait Table
where
	Self: Object,
{
	/// The name of this table.
	fn name(&self) -> &str;

	/// Search for objects based on a name and/or tags.
	fn query(self: Arc<Self>, path: &[u8]) -> Ticket<Box<dyn Query>>;

	/// Open a single object based on path.
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>>;

	/// Create a new object with the given path.
	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>>;

	fn take_job(self: Arc<Self>, timeout: Duration) -> JobTask {
		unimplemented!()
	}

	fn finish_job(self: Arc<Self>, job: JobResult, job_id: JobId) -> Result<(), ()> {
		unimplemented!()
	}

	fn cancel_job(self: Arc<Self>, job_id: JobId) {
		unimplemented!()
	}
}

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

/// A single object.
pub trait Object {
	/// Create a memory object to interact with this object. May be `None` if this object cannot
	/// be accessed directly through memory operations.
	fn memory_object(&self, _offset: u64) -> Option<Box<dyn MemoryObject>> {
		None
	}

	fn read(&self, _offset: u64, _length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn write(&self, _offset: u64, _data: &[u8]) -> Ticket<usize> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
	}

	fn seek(&self, _from: norostb_kernel::io::SeekFrom) -> Ticket<u64> {
		Ticket::new_complete(Err(Error::new(0, "not implemented".into())))
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
#[derive(Debug)]
pub enum JobRequest {
	Open { path: Box<[u8]> },
	Create { path: Box<[u8]> },
	Read { handle: Handle, amount: usize },
	Write { handle: Handle, data: Box<[u8]> },
	Seek { handle: Handle, from: SeekFrom },
	Query { filter: Box<[u8]> },
	QueryNext { handle: Handle },
}

/// A finished job.
#[derive(Debug)]
pub enum JobResult {
	Open { handle: Handle },
	Create { handle: Handle },
	Read { data: Box<[u8]> },
	Write { amount: usize },
	Seek { position: u64 },
	Query { handle: Handle },
	QueryNext { path: Box<[u8]> },
}

/// An error that occured during a job.
#[derive(Debug)]
pub struct Error {
	pub code: u32,
	pub message: Box<str>,
}

impl Error {
	pub fn new(code: u32, message: Box<str>) -> Self {
		Self { code, message }
	}
}

/// A ticket referring to a job to be completed.
#[derive(Default)]
pub struct Ticket<T> {
	inner: Arc<SpinLock<TicketInner<T>>>,
}

impl<T> Ticket<T> {
	pub fn new_complete(status: Result<T, Error>) -> Self {
		let inner = SpinLock::new(TicketInner {
			waker: None,
			status: Some(status),
		})
		.into();
		Self { inner }
	}

	pub fn new() -> (Self, TicketWaker<T>) {
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

pub struct TicketWaker<T> {
	inner: Arc<SpinLock<TicketInner<T>>>,
}

impl<T> TicketWaker<T> {
	pub fn complete(self, status: Result<T, Error>) {
		let mut l = self.inner.lock();
		l.waker.take().map(|w| w.wake());
		l.status = Some(status);
	}
}

#[derive(Default)]
pub struct TicketInner<T> {
	waker: Option<Waker>,
	/// The completion status of this job.
	status: Option<Result<T, Error>>,
}

impl<T> Future for Ticket<T> {
	type Output = Result<T, Error>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		let mut t = self.inner.lock();
		if let Some(s) = t.status.take() {
			return Poll::Ready(s);
		}
		t.waker = Some(cx.waker().clone());
		Poll::Pending
	}
}

/// The unique identifier of a table.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct TableId(pub u32);

bi_from!(newtype TableId <=> u32);

enum JobInner {
	Active {
		waker: Option<Waker>,
		job: Option<(JobId, JobRequest)>,
		table: Weak<dyn Table>,
	},
	Cancelled,
}

pub struct JobTask {
	shared: Arc<SpinLock<JobInner>>,
}

impl JobTask {
	pub fn new(table: Weak<dyn Table>, job: Option<(JobId, JobRequest)>) -> (Self, JobWaker) {
		let shared = Arc::new(SpinLock::new(JobInner::Active {
			waker: None,
			job,
			table,
		}));
		(
			Self {
				shared: shared.clone(),
			},
			JobWaker { shared },
		)
	}
}

impl Drop for JobTask {
	fn drop(&mut self) {
		match mem::replace(&mut *self.shared.lock(), JobInner::Cancelled) {
			JobInner::Active { job, table, .. } => {
				job.map(|job| Weak::upgrade(&table).map(|t| t.cancel_job(job.0)));
			}
			JobInner::Cancelled => (),
		}
	}
}

impl Future for JobTask {
	type Output = Result<(JobId, JobRequest), Cancelled>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		match &mut *self.shared.lock() {
			JobInner::Active { waker, job, .. } => {
				if let Some(s) = job.take() {
					Poll::Ready(Ok(s))
				} else {
					*waker = Some(cx.waker().clone());
					Poll::Pending
				}
			}
			JobInner::Cancelled => Poll::Ready(Err(Cancelled)),
		}
	}
}

pub struct JobWaker {
	shared: Arc<SpinLock<JobInner>>,
}

impl JobWaker {
	pub fn lock(&self) -> Result<JobWakerGuard<'_>, Cancelled> {
		let lock = self.shared.lock();
		match &*lock {
			JobInner::Active { .. } => Ok(JobWakerGuard(lock)),
			JobInner::Cancelled { .. } => Err(Cancelled),
		}
	}
}

pub struct JobWakerGuard<'a>(crate::sync::spinlock::Guard<'a, JobInner>);

impl JobWakerGuard<'_> {
	pub fn complete(&mut self, job: (JobId, JobRequest)) {
		match &mut *self.0 {
			JobInner::Active {
				waker, job: p_job, ..
			} => {
				*p_job = Some(job);
				waker.take().map(|w| w.wake());
			}
			JobInner::Cancelled { .. } => unreachable!(),
		}
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
	#[allow(dead_code)]
	InvalidTableId,
}

#[derive(Debug)]
pub struct CreateObjectError {
	pub message: Box<str>,
}
