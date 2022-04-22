use super::Error;
use crate::sync::SpinLock;
use alloc::sync::Arc;
use core::{
	future::Future,
	pin::Pin,
	task::{Context, Poll, Waker},
};

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
