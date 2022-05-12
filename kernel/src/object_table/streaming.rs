use super::*;
use crate::sync::Mutex;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};
use norostb_kernel::{io::SeekFrom, syscall::Handle};

pub struct StreamingTable {
	job_id_counter: AtomicU32,
	jobs: Mutex<Vec<(StreamJob, Option<AnyTicketWaker>, Vec<u8>)>>,
	tickets: Mutex<Vec<(JobId, AnyTicketWaker, Vec<u8>)>>,
	job_handlers: Mutex<Vec<JobWaker>>,
}

impl StreamingTable {
	pub fn new() -> Arc<Self> {
		Arc::new(Self {
			job_id_counter: Default::default(),
			jobs: Default::default(),
			tickets: Default::default(),
			job_handlers: Default::default(),
		})
	}

	fn submit_job<T>(&self, job: JobRequest, prefix: Vec<u8>) -> Ticket<T>
	where
		AnyTicketWaker: From<TicketWaker<T>>,
	{
		let (ticket, ticket_waker) = Ticket::new();

		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed);

		loop {
			let j = self.job_handlers.lock().pop();
			if let Some(w) = j {
				if let Ok(mut w) = w.lock() {
					self.tickets
						.lock()
						.push((job_id, ticket_waker.into(), prefix));
					w.complete((job_id, job));
					break;
				}
			} else {
				let mut l = self.jobs.lock();
				l.push((StreamJob { job_id, job }, Some(ticket_waker.into()), prefix));
				break;
			}
		}

		ticket.into()
	}

	/// Submit a job for which no response is expected, i.e. `finish_job` should *not* be called.
	fn submit_oneway_job(&self, job: JobRequest) {
		// Perhaps not strictly necessary, but let's try to prevent potential confusion.
		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed);

		loop {
			let j = self.job_handlers.lock().pop();
			if let Some(w) = j {
				if let Ok(mut w) = w.lock() {
					w.complete((job_id, job));
					break;
				}
			} else {
				let mut l = self.jobs.lock();
				l.push((StreamJob { job_id, job }, None, Vec::new()));
				break;
			}
		}
	}
}

impl Table for StreamingTable {
	fn take_job(self: Arc<Self>, _timeout: Duration) -> JobTask {
		let job = self.jobs.lock().pop().map(|(job, tkt, prefix)| {
			tkt.map(|tkt| self.tickets.lock().push((job.job_id, tkt, prefix)));
			(job.job_id, job.job)
		});
		let s = Arc::downgrade(&self);
		let (job, waker) = JobTask::new(s, job);
		self.job_handlers.lock().push(waker);
		job
	}

	fn finish_job(self: Arc<Self>, job: Result<JobResult, Error>, job_id: JobId) -> Result<(), ()> {
		let (tw, mut prefix);
		{
			let mut c = self.tickets.lock();
			let mut c = c.drain_filter(|e| e.0 == job_id);
			(_, tw, prefix) = c.next().ok_or(())?;
			assert!(c.next().is_none());
		}
		match job {
			Ok(JobResult::Open { handle }) | Ok(JobResult::Create { handle }) => {
				let obj = Arc::new(StreamObject {
					handle,
					table: Arc::downgrade(&self),
				});
				tw.into_object().complete(Ok(obj))
			}
			Ok(JobResult::Write { amount }) => tw.into_usize().complete(Ok(amount)),
			Ok(JobResult::Read { data }) | Ok(JobResult::Peek { data }) => {
				tw.into_data().complete(Ok(data))
			}
			Ok(JobResult::Query { handle }) => {
				tw.into_query().complete(Ok(Box::new(StreamQuery {
					table: Arc::downgrade(&self),
					handle,
					prefix,
				})))
			}
			Ok(JobResult::QueryNext { path }) => {
				tw.into_query_result().complete(if path.len() > 0 {
					prefix.extend(path.into_vec());
					Ok(QueryResult {
						path: prefix.into(),
					})
				} else {
					// FIXME query API sucks
					Err(Error::InvalidOperation)
				});
			}
			Ok(JobResult::Seek { position }) => {
				tw.into_u64().complete(Ok(position));
			}
			Err(e) => tw.complete_err(e),
		}
		Ok(())
	}
}

impl Object for StreamingTable {
	fn query(self: Arc<Self>, prefix: Vec<u8>, filter: &[u8]) -> Ticket<Box<dyn Query>> {
		self.submit_job(
			JobRequest::Query {
				handle: Handle::MAX,
				filter: filter.into(),
			},
			prefix.into(),
		)
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			JobRequest::Open {
				handle: Handle::MAX,
				path: path.into(),
			},
			Default::default(),
		)
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			JobRequest::Create {
				handle: Handle::MAX,
				path: path.into(),
			},
			Default::default(),
		)
	}

	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		Some(self)
	}
}

impl Drop for StreamingTable {
	fn drop(&mut self) {
		// Wake any waiting tasks so they don't get stuck endlessly.
		for task in self
			.jobs
			.get_mut()
			.drain(..)
			.flat_map(|e| e.1)
			.chain(self.tickets.get_mut().drain(..).map(|e| e.1))
		{
			task.complete_err(Error::Cancelled)
		}
	}
}

struct StreamObject {
	handle: Handle,
	table: Weak<StreamingTable>,
}

impl StreamObject {
	fn submit_job<T>(&self, job: impl FnOnce() -> JobRequest, prefix: Vec<u8>) -> Ticket<T>
	where
		AnyTicketWaker: From<TicketWaker<T>>,
	{
		self.table.upgrade().map_or_else(
			|| Ticket::new_complete(Err(Error::Cancelled)),
			|tbl| tbl.submit_job(job(), prefix.into()),
		)
	}
}

impl Object for StreamObject {
	fn query(self: Arc<Self>, prefix: Vec<u8>, filter: &[u8]) -> Ticket<Box<dyn Query>> {
		self.submit_job(
			|| JobRequest::Query {
				handle: self.handle,
				filter: filter.into(),
			},
			prefix.into(),
		)
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			|| JobRequest::Open {
				handle: self.handle,
				path: path.into(),
			},
			Default::default(),
		)
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			|| JobRequest::Create {
				handle: self.handle,
				path: path.into(),
			},
			Default::default(),
		)
	}

	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		self.submit_job(
			|| JobRequest::Read {
				handle: self.handle,
				amount: length,
			},
			Default::default(),
		)
	}

	fn peek(&self, length: usize) -> Ticket<Box<[u8]>> {
		self.submit_job(
			|| JobRequest::Peek {
				handle: self.handle,
				amount: length,
			},
			Default::default(),
		)
	}

	fn write(&self, data: &[u8]) -> Ticket<usize> {
		self.submit_job(
			|| JobRequest::Write {
				handle: self.handle,
				data: data.into(),
			},
			Default::default(),
		)
	}

	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		self.submit_job(
			|| JobRequest::Seek {
				handle: self.handle,
				from,
			},
			Default::default(),
		)
	}
}

impl Drop for StreamObject {
	fn drop(&mut self) {
		Weak::upgrade(&self.table).map(|table| {
			table.submit_oneway_job(JobRequest::Close {
				handle: self.handle,
			});
		});
	}
}

struct StreamQuery {
	table: Weak<StreamingTable>,
	handle: Handle,
	prefix: Vec<u8>,
}

impl Iterator for StreamQuery {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		Weak::upgrade(&self.table).map(|table| {
			table.submit_job(
				JobRequest::QueryNext {
					handle: self.handle,
				},
				self.prefix.clone(),
			)
		})
	}
}

impl Query for StreamQuery {}

impl Drop for StreamQuery {
	fn drop(&mut self) {
		todo!()
	}
}

struct StreamJob {
	job_id: JobId,
	job: JobRequest,
}
