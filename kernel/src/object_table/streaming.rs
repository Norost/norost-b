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
	jobs: Mutex<Vec<(JobId, StreamJob, TicketWaker)>>,
	tickets: Mutex<Vec<(JobId, TicketWaker)>>,
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

	fn submit_job(&self, job: StreamJob) -> Ticket {
		let (ticket, ticket_waker) = Ticket::new();

		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed).into();

		loop {
			let j = self.job_handlers.lock().pop();
			if let Some(w) = j {
				if let Ok(mut w) = w.lock() {
					self.tickets.lock().push((job_id, ticket_waker));
					w.complete(job.into_job(job_id));
					break;
				}
			} else {
				let mut l = self.jobs.lock();
				l.push((job_id, job, ticket_waker));
				break;
			}
		}

		ticket
	}
}

impl Table for StreamingTable {
	fn name(&self) -> &str {
		&self.name
	}

	fn query(self: Arc<Self>, tags: &[&str]) -> Box<dyn Query> {
		todo!();
		let tags = {
			let l = 2 + tags.len() * 2 + tags.iter().map(|s| s.len()).sum::<usize>();
			let mut t = Vec::with_capacity(l);
			let [a, b] = u16::try_from(tags.len()).unwrap().to_ne_bytes();
			t.push(a);
			t.push(b);
			let mut s = u16::try_from(2 + tags.len() * 2).unwrap();
			for tag in tags {
				s += u16::try_from(tag.len()).unwrap();
				let [a, b] = s.to_ne_bytes();
				t.push(a);
				t.push(b);
			}
			tags.iter().for_each(|s| t.extend(s.as_bytes()));
			t.into()
		};
		let job = self.submit_job(StreamJob::Query { tags });
		Box::new(StreamQuery {
	//		job,
		})
	}

	fn get(self: Arc<Self>, id: Id) -> Ticket {
		self.submit_job(StreamJob::Open { object_id: id })
	}

	fn create(self: Arc<Self>, tags: &[u8]) -> Ticket {
		let tags = tags.into();
		self.submit_job(StreamJob::Create { tags })
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
		let mut c = self.tickets.lock();
		let mut c = c.drain_filter(|e| e.0 == job.job_id);
		let (_, tw) = c.next().ok_or(())?;
		match job.ty {
			JobType::Open => {
				let obj = Arc::new(StreamObject {
					id: job.object_id,
					table: Arc::downgrade(&self),
				});
				tw.complete(Ok(Data::Object(obj)));
			}
			JobType::Write => {
				tw.complete(Ok(Data::Usize(job.operation_size.try_into().unwrap())));
			}
			JobType::Read => {
				tw.complete(Ok(Data::Bytes(job.buffer)));
			}
			JobType::Query => {
				todo!()
			}
			JobType::Create => {
				let obj = Arc::new(StreamObject {
					id: job.object_id,
					table: Arc::downgrade(&self),
				});
				tw.complete(Ok(Data::Object(obj)));
			}
		}
		assert!(c.next().is_none());
		Ok(())
	}

	fn cancel_job(self: Arc<Self>, job: Job) {
		// Re-queue
		self.submit_job(job.into());
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
	fn read(&self, _: u64, length: u32) -> Result<Ticket, ()> {
		self.table
			.upgrade()
			.map(|tbl| {
				let job = StreamJob::Read {
					object_id: self.id,
					length,
				};
				tbl.submit_job(job)
			})
			.ok_or(())
	}

	fn write(&self, _: u64, data: &[u8]) -> Result<Ticket, ()> {
		self.table
			.upgrade()
			.map(|tbl| {
				let job = StreamJob::Write {
					object_id: self.id,
					data: data.into(),
				};
				tbl.submit_job(job)
			})
			.ok_or(())
	}
}

enum StreamJob {
	Open { object_id: Id },
	Read { object_id: Id, length: u32 },
	Write { object_id: Id, data: Box<[u8]> },
	Query { tags: Box<[u8]> },
	Create { tags: Box<[u8]> },
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
			StreamJob::Query { .. } => todo!(),
			StreamJob::Create { tags } => Job {
				ty: JobType::Create,
				job_id,
				operation_size: tags.len().try_into().unwrap(),
				buffer: tags,
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
		}: Job,
	) -> Self {
		match ty {
			JobType::Open => StreamJob::Open { object_id },
			JobType::Read => todo!(),
			JobType::Write => StreamJob::Write {
				object_id,
				data: buffer,
			},
			JobType::Query => StreamJob::Query { tags: buffer },
			JobType::Create => StreamJob::Create { tags: buffer },
		}
	}
}

struct StreamQueryInner {}

struct StreamQuery {
	//inner: Arc<StreamQueryInner>,
}

impl Iterator for StreamQuery {
	type Item = QueryResult;

	fn next(&mut self) -> Option<Self::Item> {
		None
	}
}

impl Query for StreamQuery {}

struct QueryHandle(u32);
