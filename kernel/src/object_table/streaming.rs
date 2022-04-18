use super::*;
use crate::sync::Mutex;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};
use norostb_kernel::{io::SeekFrom, syscall::Handle};

#[derive(Default)]
pub struct StreamingTable {
	name: Box<str>,
	job_id_counter: AtomicU32,
	jobs: Mutex<Vec<(StreamJob, StreamTicketWaker)>>,
	tickets: Mutex<Vec<(JobId, StreamTicketWaker)>>,
	job_handlers: Mutex<Vec<JobWaker>>,
}

impl StreamingTable {
	pub fn new(name: Box<str>) -> Arc<Self> {
		Arc::new(Self {
			name,
			..Default::default()
		})
	}

	fn submit_job<T>(&self, job: JobRequest) -> Ticket<T>
	where
		StreamTicketWaker: From<TicketWaker<T>>,
	{
		let (ticket, ticket_waker) = Ticket::new();

		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed);

		loop {
			let j = self.job_handlers.lock().pop();
			if let Some(w) = j {
				if let Ok(mut w) = w.lock() {
					self.tickets.lock().push((job_id, ticket_waker.into()));
					w.complete((job_id, job));
					break;
				}
			} else {
				let mut l = self.jobs.lock();
				l.push((StreamJob { job_id, job }, ticket_waker.into()));
				break;
			}
		}

		ticket.into()
	}
}

impl Table for StreamingTable {
	fn name(&self) -> &str {
		&self.name
	}

	fn query(self: Arc<Self>, filter: &[u8]) -> Ticket<Box<dyn Query>> {
		self.submit_job(JobRequest::Query {
			filter: filter.into(),
		})
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(JobRequest::Open { path: path.into() })
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(JobRequest::Create { path: path.into() })
	}

	fn take_job(self: Arc<Self>, _timeout: Duration) -> JobTask {
		let job = self.jobs.lock().pop().map(|(job, tkt)| {
			self.tickets.lock().push((job.job_id, tkt));
			(job.job_id, job.job)
		});
		let s = Arc::downgrade(&self);
		let (job, waker) = JobTask::new(s, job);
		self.job_handlers.lock().push(waker);
		job
	}

	fn finish_job(self: Arc<Self>, job: JobResult, job_id: JobId) -> Result<(), ()> {
		let tw;
		{
			let mut c = self.tickets.lock();
			let mut c = c.drain_filter(|e| e.0 == job_id);
			(_, tw) = c.next().ok_or(())?;
			assert!(c.next().is_none());
		}
		match job {
			JobResult::Open { handle } | JobResult::Create { handle } => {
				let obj = Arc::new(StreamObject {
					handle,
					table: Arc::downgrade(&self),
				});
				tw.into_object().complete(Ok(obj))
			}
			JobResult::Write { amount } => tw.into_usize().complete(Ok(amount)),
			JobResult::Read { data } => tw.into_data().complete(Ok(data)),
			JobResult::Query { handle } => tw.into_query().complete(Ok(Box::new(StreamQuery {
				table: self,
				handle,
			}))),
			JobResult::QueryNext { path } => {
				tw.into_query_result().complete(if path.len() > 0 {
					Ok(QueryResult { path })
				} else {
					Err(Error::new(0, "".into()))
				});
			}
			JobResult::Seek { position } => {
				tw.into_u64().complete(Ok(position));
			}
		}
		Ok(())
	}
}

impl Object for StreamingTable {
	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		Some(self)
	}
}

struct StreamObject {
	handle: Handle,
	table: Weak<StreamingTable>,
}

impl Object for StreamObject {
	fn read(&self, _: u64, length: usize) -> Ticket<Box<[u8]>> {
		self.table
			.upgrade()
			.map(|tbl| {
				tbl.submit_job(JobRequest::Read {
					handle: self.handle,
					amount: length,
				})
			})
			.unwrap_or_else(|| {
				Ticket::new_complete(Err(Error::new(1, "TODO error message".into())))
			})
	}

	fn write(&self, _: u64, data: &[u8]) -> Ticket<usize> {
		self.table
			.upgrade()
			.map(|tbl| {
				tbl.submit_job(JobRequest::Write {
					handle: self.handle,
					data: data.into(),
				})
			})
			.unwrap_or_else(|| {
				Ticket::new_complete(Err(Error::new(1, "TODO error message".into())))
			})
	}

	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		self.table
			.upgrade()
			.map(|tbl| {
				tbl.submit_job(JobRequest::Seek {
					handle: self.handle,
					from,
				})
			})
			.unwrap_or_else(|| {
				Ticket::new_complete(Err(Error::new(1, "TODO error message".into())))
			})
	}
}

struct StreamQuery {
	table: Arc<StreamingTable>,
	handle: Handle,
}

impl Iterator for StreamQuery {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		Some(self.table.submit_job(JobRequest::QueryNext {
			handle: self.handle,
		}))
	}
}

impl Query for StreamQuery {}

impl Drop for StreamQuery {
	fn drop(&mut self) {
		todo!()
	}
}

enum StreamTicketWaker {
	Object(TicketWaker<Arc<dyn Object>>),
	Usize(TicketWaker<usize>),
	U64(TicketWaker<u64>),
	Data(TicketWaker<Box<[u8]>>),
	Query(TicketWaker<Box<dyn Query>>),
	QueryResult(TicketWaker<QueryResult>),
}

macro_rules! stream_ticket {
	($t:ty => $v:ident, $f:ident) => {
		impl From<TicketWaker<$t>> for StreamTicketWaker {
			fn from(t: TicketWaker<$t>) -> Self {
				Self::$v(t)
			}
		}

		impl StreamTicketWaker {
			#[track_caller]
			fn $f(self) -> TicketWaker<$t> {
				match self {
					Self::$v(t) => t,
					_ => unreachable!(),
				}
			}
		}
	};
}
stream_ticket!(Arc<dyn Object> => Object, into_object);
stream_ticket!(usize => Usize, into_usize);
stream_ticket!(u64 => U64, into_u64);
stream_ticket!(Box<[u8]> => Data, into_data);
stream_ticket!(Box<dyn Query> => Query, into_query);
stream_ticket!(QueryResult => QueryResult, into_query_result);

struct StreamJob {
	job_id: JobId,
	job: JobRequest,
}
