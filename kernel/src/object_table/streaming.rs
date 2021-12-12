use super::*;
use crate::sync::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};
use alloc::{boxed::Box, sync::{Arc, Weak}, vec::Vec};

#[derive(Default)]
pub struct StreamingTable {
	name: Box<str>,
	//event_wakers: Mutex<(usize, Vec<EventWaker>)>,
	job_id_counter: AtomicU32,
	jobs: Mutex<Vec<(JobId, StreamJob, Id, TicketWaker)>>,
	tickets: Mutex<Vec<(JobId, Id, TicketWaker)>>,
	job_handlers: Mutex<Vec<JobWaker>>,
}

impl StreamingTable {
	pub fn new(name: Box<str>) -> Arc<Self> {
		Self { name, ..Default::default() }.into()
	}

	fn submit_job(&self, job: StreamJob, id: Id) -> Ticket {
		let (ticket, ticket_waker) = Ticket::new();

		let job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed).into();

		let j = self.job_handlers.lock().pop();
		if let Some(w) = j {
			self.tickets.lock().push((job_id, id, ticket_waker));
			w.complete(job.into_job(job_id, id));
		} else {
			let mut l = self.jobs.lock();
			l.push((job_id, job, id, ticket_waker));
		}

		ticket
	}
}

impl Table for StreamingTable {
	fn name(&self) -> &str {
		&self.name
	}

	fn query(self: Arc<Self>, _name: Option<&str>, _tags: &[&str]) -> Box<dyn Query> {
		todo!()
	}

	fn get(self: Arc<Self>, id: Id) -> Ticket {
		self.submit_job(StreamJob::Open, id)
	}

	fn create(self: Arc<Self>, _name: &str, _tags: &[&str]) -> Ticket {
		todo!()
	}

	fn take_job(&self) -> JobTask {
		let job = self.jobs.lock().pop().map(|(job_id, job, id, tkt)| {
			self.tickets.lock().push((job_id, id, tkt));
			job.into_job(job_id, id)
		});
		let (job, waker) = JobTask::new(job);
		self.job_handlers.lock().push(waker);
		job
	}

	fn finish_job(self: Arc<Self>, job: Job) -> Result<(), ()> {
		let mut c = self.tickets.lock();
		let mut c = c.drain_filter(|e| e.0 == job.job_id);
		let (_, id, tw) = c.next().ok_or(())?;
		match job.ty {
			JobType::Open => {
				let obj = Arc::new(StreamObject { id, table: Arc::downgrade(&self) });
				tw.complete(Ok(Data::Object(obj)));
			}
			JobType::Write => {
				tw.complete(Ok(Data::Usize(job.operation_size.try_into().unwrap())));
			},
			JobType::Read => todo!(),
		}
		assert!(c.next().is_none());
		Ok(())
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
	fn read(&self, _: u64, _data: &mut [u8]) -> Result<Ticket, ()> {
		todo!();
	}

	fn write(&self, _: u64, data: &[u8]) -> Result<Ticket, ()> {
		self.table.upgrade().map(|tbl| {
			let job = StreamJob::Write { data: data.into() };
			tbl.submit_job(job, self.id)
		}).ok_or(())
	}
}

enum StreamJob {
	Open,
	Write { data: Box<[u8]> },
}

impl StreamJob {
	fn into_job(self, job_id: JobId, object_id: Id) -> Job {
		match self {
			StreamJob::Open => Job {
				ty: JobType::Open,
				job_id,
				object_id,
				..Default::default()
			},
			StreamJob::Write { data } => {
				Job {
					ty: JobType::Write,
					job_id,
					object_id,
					operation_size: data.len().try_into().unwrap(),
					buffer: data,
					..Default::default()
				}
			}
		}
	}
}
