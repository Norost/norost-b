use super::{Handle, Table};
use crate::sync::SpinLock;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::{
	future::Future,
	mem,
	pin::Pin,
	task::{Context, Poll, Waker},
};

pub use norostb_kernel::io::{JobId, SeekFrom};

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
	Close { handle: Handle },
	Peek { handle: Handle, amount: usize },
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
	Peek { data: Box<[u8]> },
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

pub struct Cancelled;
