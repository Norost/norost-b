use super::*;
use crate::sync::SpinLock;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use arena::Arena;
use core::{
	mem, str,
	sync::atomic::{AtomicU32, Ordering},
};
use norostb_kernel::{
	io::{Job, SeekFrom},
	syscall::Handle,
};

pub struct StreamingTable {
	job_id_counter: AtomicU32,
	jobs: SpinLock<Vec<(Box<[u8]>, Option<AnyTicketWaker>)>>,
	tickets: SpinLock<Vec<(JobId, AnyTicketWaker)>>,
	job_handlers: SpinLock<Vec<(TicketWaker<Box<[u8]>>, usize)>>,
	/// Objects that are being shared.
	shared: SpinLock<Arena<Arc<dyn Object>, ()>>,
}

/// A wrapper around a [`StreamingTable`], intended for owners to process jobs.
#[repr(transparent)]
pub struct StreamingTableOwner(StreamingTable);

impl StreamingTableOwner {
	pub fn new() -> Arc<Self> {
		Arc::new(Self(StreamingTable {
			job_id_counter: Default::default(),
			jobs: Default::default(),
			tickets: Default::default(),
			job_handlers: Default::default(),
			shared: Default::default(),
		}))
	}

	pub fn into_inner_weak(slf: &Arc<Self>) -> Weak<StreamingTable> {
		// SAFETY: StreamingTableOwner is a transparent wrapper around
		// StreamingTable
		unsafe { Weak::from_raw(Weak::into_raw(Arc::downgrade(slf)).cast::<StreamingTable>()) }
	}
}

impl StreamingTable {
	fn submit_job<T>(&self, mut job: Job, data: &[u8]) -> Ticket<T>
	where
		AnyTicketWaker: From<TicketWaker<T>>,
	{
		let (ticket, ticket_waker) = Ticket::new();

		job.job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed);
		let job_id = job.job_id;
		let job = job.as_ref().iter().chain(data).copied().collect::<Box<_>>();

		if let Some(w) = self.job_handlers.auto_lock().pop() {
			self.tickets.auto_lock().push((job_id, ticket_waker.into()));
			w.0.complete(Ok(job));
		} else {
			self.jobs.auto_lock().push((job, Some(ticket_waker.into())));
		}

		ticket.into()
	}

	/// Submit a job for which no response is expected, i.e. `finish_job` should *not* be called.
	fn submit_oneway_job(&self, mut job: Job) {
		// Perhaps not strictly necessary, but let's try to prevent potential confusion.
		job.job_id = self.job_id_counter.fetch_add(1, Ordering::Relaxed);
		let job = (*job.as_ref()).into();

		if let Some(w) = self.job_handlers.auto_lock().pop() {
			w.0.complete(Ok(job));
		} else {
			self.jobs.auto_lock().push((job, None));
		}
	}
}

impl Object for StreamingTableOwner {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(
			str::from_utf8(path)
				.ok()
				.and_then(|s| s.parse::<u32>().ok())
				.map(|h| arena::Handle::from_raw(h as usize, ()))
				.and_then(|h| self.0.shared.auto_lock().remove(h))
				.ok_or(Error::InvalidData),
		)
	}

	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		if length < mem::size_of::<Job>() + 8 {
			return Ticket::new_complete(Err(Error::InvalidData));
		}
		match self.0.jobs.auto_lock().pop() {
			Some((job, waker)) => {
				if let Some(w) = waker {
					let id = Job::deserialize(&job).unwrap().0.job_id;
					self.0.tickets.auto_lock().push((id, w));
				}
				Ticket::new_complete(Ok(job))
			}
			None => {
				let (ticket, waker) = Ticket::new();
				self.0
					.job_handlers
					.auto_lock()
					.push((waker, length - mem::size_of::<Job>()));
				ticket
			}
		}
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<usize> {
		let Some((job, data)) = Job::deserialize(data) else {
			return Ticket::new_complete(Err(Error::InvalidData));
		};
		let mut c = self.0.tickets.auto_lock();
		let mut c = c.drain_filter(|e| e.0 == job.job_id);
		let Some((_, tw)) = c.next() else {
			return Ticket::new_complete(Err(Error::InvalidObject));
		};
		assert!(c.next().is_none());
		match job.ty {
			_ if job.result != 0 => tw.complete_err(Error::from(job.result)),
			Job::OPEN | Job::OPEN_SHARE | Job::CREATE => {
				tw.into_object().complete(Ok(if job.ty == Job::OPEN_SHARE {
					// FIXME we don't guarantee this is the correct process
					crate::scheduler::process::Process::current()
						.unwrap()
						.object_apply(job.handle, |o| o.clone())
						.unwrap_or_else(|| todo!())
				} else {
					Arc::new(StreamObject {
						handle: job.handle,
						table: Self::into_inner_weak(&self),
					})
				}))
			}
			Job::WRITE => {
				let Ok(len) = data.try_into().map(|a| u64::from_ne_bytes(a)) else {
					return Ticket::new_complete(Err(Error::InvalidData));
				};
				tw.into_usize().complete(Ok(len as usize))
			}
			Job::READ | Job::PEEK => tw.into_data().complete(Ok(data.into())),
			Job::SEEK => {
				let Ok(offset) = data.try_into().map(|a| u64::from_ne_bytes(a)) else {
					return Ticket::new_complete(Err(Error::InvalidData));
				};
				tw.into_u64().complete(Ok(offset))
			}
			Job::SHARE => tw.into_u64().complete(Ok(0)),
			_ => return Ticket::new_complete(Err(Error::InvalidOperation)),
		}
		Ticket::new_complete(Ok(data.len()))
	}
}

impl Object for StreamingTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			Job {
				ty: Job::OPEN,
				handle: Handle::MAX,
				..Default::default()
			},
			path,
		)
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			Job {
				ty: Job::CREATE,
				handle: Handle::MAX,
				..Default::default()
			},
			path,
		)
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
	fn submit_job<T>(&self, job: Job, data: &[u8]) -> Ticket<T>
	where
		AnyTicketWaker: From<TicketWaker<T>>,
	{
		self.table.upgrade().map_or_else(
			|| Ticket::new_complete(Err(Error::Cancelled)),
			|tbl| tbl.submit_job(job, data),
		)
	}
}

impl Object for StreamObject {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			Job {
				ty: Job::OPEN,
				handle: self.handle,
				..Default::default()
			},
			path,
		)
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(
			Job {
				ty: Job::CREATE,
				handle: self.handle,
				..Default::default()
			},
			path,
		)
	}

	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		self.submit_job(
			Job {
				ty: Job::READ,
				handle: self.handle,
				..Default::default()
			},
			&(length as u64).to_ne_bytes(),
		)
	}

	fn peek(&self, length: usize) -> Ticket<Box<[u8]>> {
		self.submit_job(
			Job {
				ty: Job::PEEK,
				handle: self.handle,
				..Default::default()
			},
			&(length as u64).to_ne_bytes(),
		)
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<usize> {
		self.submit_job(
			Job {
				ty: Job::WRITE,
				handle: self.handle,
				..Default::default()
			},
			data,
		)
	}

	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		let (from_anchor, from_offset) = from.into_raw();
		self.submit_job(
			Job {
				ty: Job::SEEK,
				handle: self.handle,
				from_anchor,
				..Default::default()
			},
			&from_offset.to_ne_bytes(),
		)
	}

	fn share(&self, share: &Arc<dyn Object>) -> Ticket<u64> {
		match self.table.upgrade() {
			None => Ticket::new_complete(Err(Error::Cancelled)),
			Some(tbl) => {
				let share = tbl.shared.auto_lock().insert(share.clone());
				tbl.submit_job(
					Job {
						ty: Job::SHARE,
						handle: self.handle,
						..Default::default()
					},
					&u32::try_from(share.into_raw().0).unwrap().to_ne_bytes(),
				)
			}
		}
	}
}

impl Drop for StreamObject {
	fn drop(&mut self) {
		Weak::upgrade(&self.table).map(|table| {
			table.submit_oneway_job(Job {
				ty: Job::CLOSE,
				handle: self.handle,
				..Default::default()
			});
		});
	}
}
