use super::*;
use crate::sync::Mutex;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Default)]
pub struct StreamingTable {
	name: Box<str>,
	//event_wakers: Mutex<(usize, Vec<EventWaker>)>,
	job_id_counter: AtomicU32,
	jobs: Mutex<Vec<(JobId, StreamJob, StreamTicketWaker)>>,
	tickets: Mutex<Vec<(JobId, StreamTicketWaker)>>,
	job_handlers: Mutex<Vec<JobWaker>>,
	/// A self reference is necessary for taking/finishing jos, which correspond to read/write
	/// provided by the Object trait. This trait uses `&self` and not `self: Arc<Self>` since
	/// the latter would add a non-trivial cost for what is presumably not needed very often.
	self_ref: Weak<Self>,
}

impl StreamingTable {
	pub fn new(name: Box<str>) -> Arc<Self> {
		Arc::new_cyclic(|self_ref| Self {
			name,
			self_ref: self_ref.clone(),
			..Default::default()
		})
	}

	fn submit_job<T>(&self, job: StreamJob) -> Ticket<T>
	where
		StreamTicketWaker: From<TicketWaker<T>>,
	{
		let (ticket, ticket_waker) = Ticket::new();

		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed).into();

		loop {
			let j = self.job_handlers.lock().pop();
			if let Some(w) = j {
				if let Ok(mut w) = w.lock() {
					self.tickets.lock().push((job_id, ticket_waker.into()));
					w.complete(job.into_job(job_id));
					break;
				}
			} else {
				let mut l = self.jobs.lock();
				l.push((job_id, job, ticket_waker.into()));
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

	fn query(self: Arc<Self>, path: &[u8]) -> Ticket<Box<dyn Query>> {
		self.submit_job(StreamJob::Query { path: path.into() })
	}

	fn get(self: Arc<Self>, id: Id) -> Ticket<Arc<dyn Object>> {
		self.submit_job(StreamJob::Open { object_id: id })
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(StreamJob::Create { path: path.into() })
	}

	fn take_job(self: Arc<Self>, timeout: Duration) -> JobTask {
		let job = self.jobs.lock().pop().map(|(job_id, job, tkt)| {
			self.tickets.lock().push((job_id, tkt));
			job.into_job(job_id)
		});
		let s = Arc::downgrade(&self);
		let (job, waker) = JobTask::new(s, job);
		self.job_handlers.lock().push(waker);
		job
	}

	fn finish_job(self: Arc<Self>, job: Job) -> Result<(), ()> {
		let tw;
		{
			let mut c = self.tickets.lock();
			let mut c = c.drain_filter(|e| e.0 == job.job_id);
			(_, tw) = c.next().ok_or(())?;
			assert!(c.next().is_none());
		}
		match job.ty {
			JobType::Open => {
				let obj = Arc::new(StreamObject {
					id: job.object_id,
					table: Arc::downgrade(&self),
				});
				tw.into_object().complete(Ok(obj));
			}
			JobType::Write => {
				tw.into_usize()
					.complete(Ok(job.operation_size.try_into().unwrap()));
			}
			JobType::Read => {
				tw.into_data().complete(Ok(job.buffer));
			}
			JobType::Query => {
				tw.into_query().complete(Ok(Box::new(StreamQuery {
					table: self,
					query_id: job.query_id,
				})));
			}
			JobType::QueryNext => {
				tw.into_query_result().complete(if job.operation_size > 0 {
					Ok(QueryResult {
						path: job.buffer[..job.operation_size.try_into().unwrap()].into(),
						id: job.object_id,
					})
				} else {
					Err(Error::new(0, "".into()))
				});
			}
			JobType::Create => {
				let obj = Arc::new(StreamObject {
					id: job.object_id,
					table: Arc::downgrade(&self),
				});
				tw.into_object().complete(Ok(obj));
			}
		}
		Ok(())
	}

	fn cancel_job(self: Arc<Self>, job: Job) {
		// Re-queue
		todo!();
		//self.submit_job(job.into()); // FIXME this is broken as the ticket itself is lost
	}
}

impl Object for StreamingTable {
	fn event_listener(&self) -> Result<EventListener, Unpollable> {
		/*
		let mut ew = self.jobs.lock();
		let (l, w) = EventListener::new();
		if let Some(c) = ew.0.checked_sub(1) {
			w.complete(Events(42));
			ew.0 = c;
		} else {
			ew.1.push(w);
		}
		Ok(l)
		*/
		todo!()
	}

	fn as_table(self: Arc<Self>) -> Option<Arc<dyn Table>> {
		Some(self)
	}
}

struct StreamObject {
	id: Id,
	table: Weak<StreamingTable>,
}

impl Object for StreamObject {
	fn read(&self, _: u64, length: u32) -> Ticket<Box<[u8]>> {
		self.table
			.upgrade()
			.map(|tbl| {
				let job = StreamJob::Read {
					object_id: self.id,
					length,
				};
				tbl.submit_job(job)
			})
			.unwrap_or_else(|| {
				Ticket::new_complete(Err(Error::new(1, "TODO error message".into())))
			})
	}

	fn write(&self, _: u64, data: &[u8]) -> Ticket<usize> {
		self.table
			.upgrade()
			.map(|tbl| {
				let job = StreamJob::Write {
					object_id: self.id,
					data: data.into(),
				};
				tbl.submit_job(job)
			})
			.unwrap_or_else(|| {
				Ticket::new_complete(Err(Error::new(1, "TODO error message".into())))
			})
	}
}

enum StreamJob {
	Open { object_id: Id },
	Read { object_id: Id, length: u32 },
	Write { object_id: Id, data: Box<[u8]> },
	Query { path: Box<[u8]> },
	Create { path: Box<[u8]> },
	QueryNext { query_id: QueryId },
}

impl StreamJob {
	fn into_job(self, job_id: JobId) -> Job {
		match self {
			StreamJob::Open { object_id } => Job {
				ty: JobType::Open,
				job_id,
				object_id,
				..Default::default()
			},
			StreamJob::Read { object_id, length } => Job {
				ty: JobType::Read,
				job_id,
				object_id,
				operation_size: length,
				..Default::default()
			},
			StreamJob::Write { object_id, data } => Job {
				ty: JobType::Write,
				job_id,
				object_id,
				operation_size: data.len().try_into().unwrap(),
				buffer: data,
				..Default::default()
			},
			StreamJob::Query { path } => Job {
				ty: JobType::Query,
				job_id,
				operation_size: path.len().try_into().unwrap(),
				buffer: path,
				..Default::default()
			},
			StreamJob::Create { path } => Job {
				ty: JobType::Create,
				job_id,
				operation_size: path.len().try_into().unwrap(),
				buffer: path,
				..Default::default()
			},
			StreamJob::QueryNext { query_id } => Job {
				ty: JobType::QueryNext,
				job_id,
				query_id,
				..Default::default()
			},
		}
	}
}

impl From<Job> for StreamJob {
	fn from(
		Job {
			ty,
			flags,
			job_id: _,
			operation_size,
			object_id,
			buffer,
			query_id,
		}: Job,
	) -> Self {
		match ty {
			JobType::Open => StreamJob::Open { object_id },
			JobType::Read => todo!(),
			JobType::Write => StreamJob::Write {
				object_id,
				data: buffer,
			},
			JobType::Query => StreamJob::Query { path: buffer },
			JobType::QueryNext => StreamJob::QueryNext { query_id },
			JobType::Create => StreamJob::Create { path: buffer },
		}
	}
}

struct StreamQuery {
	table: Arc<StreamingTable>,
	query_id: QueryId,
}

impl Iterator for StreamQuery {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		Some(self.table.submit_job(StreamJob::QueryNext {
			query_id: self.query_id,
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
stream_ticket!(Box<[u8]> => Data, into_data);
stream_ticket!(Box<dyn Query> => Query, into_query);
stream_ticket!(QueryResult => QueryResult, into_query_result);
