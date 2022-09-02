use {
	super::{Error, Object},
	crate::sync::SpinLock,
	alloc::{boxed::Box, sync::Arc},
	core::{
		fmt,
		future::Future,
		pin::Pin,
		task::{Context, Poll, Waker},
	},
};

/// A ticket referring to a job to be completed.
#[derive(Default)]
pub struct Ticket<T> {
	inner: Arc<SpinLock<TicketInner<T>>>,
}

impl<T> Ticket<T> {
	pub fn new_complete(status: Result<T, Error>) -> Self {
		let inner = SpinLock::new(TicketInner { waker: None, status: Some(status) }).into();
		Self { inner }
	}

	pub fn new() -> (Self, TicketWaker<T>) {
		let inner = Arc::new(SpinLock::new(TicketInner { waker: None, status: None }));
		(Self { inner: inner.clone() }, TicketWaker { inner })
	}
}

impl<T> From<Result<T, Error>> for Ticket<T> {
	fn from(res: Result<T, Error>) -> Self {
		Self::new_complete(res)
	}
}

impl<T> From<Error> for Ticket<T> {
	fn from(e: Error) -> Self {
		Self::new_complete(Err(e))
	}
}

pub struct TicketWaker<T> {
	inner: Arc<SpinLock<TicketInner<T>>>,
}

impl<T> TicketWaker<T> {
	#[cfg_attr(debug_assertions, track_caller)]
	pub fn complete(self, status: Result<T, Error>) {
		let mut l = self.inner.lock();
		l.waker.take().map(|w| w.wake());
		l.status = Some(status);
	}

	pub fn isr_complete(self, status: Result<T, Error>) {
		let mut l = self.inner.isr_lock();
		l.waker.take().map(|w| w.wake());
		l.status = Some(status);
	}
}

impl<T: fmt::Debug> fmt::Debug for TicketWaker<T> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		(&*self.inner.auto_lock()).fmt(f)
	}
}

#[derive(Debug, Default)]
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

/// An enum that can hold the common ticket types.
pub enum AnyTicket {
	Object(Ticket<Arc<dyn Object>>),
	U64(Ticket<u64>),
	Data(Ticket<Box<[u8]>>),
}

/// An enum that can hold the common ticket waker types.
pub enum AnyTicketWaker {
	Object(TicketWaker<Arc<dyn Object>>),
	U64(TicketWaker<u64>),
	Data(TicketWaker<Box<[u8]>>),
}

/// An enum that can hold the common ticket result types.
pub enum AnyTicketValue {
	Object(Arc<dyn Object>),
	U64(u64),
	Data(Box<[u8]>),
}

macro_rules! any_ticket {
	($t:ty => $v:ident) => {
		impl From<Ticket<$t>> for AnyTicket {
			fn from(t: Ticket<$t>) -> Self {
				Self::$v(t)
			}
		}

		impl From<TicketWaker<$t>> for AnyTicketWaker {
			fn from(t: TicketWaker<$t>) -> Self {
				Self::$v(t)
			}
		}

		impl From<$t> for AnyTicketValue {
			fn from(t: $t) -> Self {
				Self::$v(t)
			}
		}

		impl From<$t> for Ticket<$t> {
			fn from(v: $t) -> Self {
				Self::new_complete(Ok(v))
			}
		}
	};
}
any_ticket!(Arc<dyn Object> => Object);
any_ticket!(u64 => U64);
any_ticket!(Box<[u8]> => Data);

impl AnyTicketWaker {
	pub fn complete_err(self, err: Error) {
		match self {
			Self::Object(t) => t.complete(Err(err)),
			Self::U64(t) => t.complete(Err(err)),
			Self::Data(t) => t.complete(Err(err)),
		}
	}

	pub fn isr_complete_err(self, err: Error) {
		match self {
			Self::Object(t) => t.isr_complete(Err(err)),
			Self::U64(t) => t.isr_complete(Err(err)),
			Self::Data(t) => t.isr_complete(Err(err)),
		}
	}
}

impl Future for AnyTicket {
	type Output = Result<AnyTicketValue, Error>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
		match &mut *self {
			Self::Object(t) => Pin::new(t).poll(cx).map(|r| r.map(Into::into)),
			Self::U64(t) => Pin::new(t).poll(cx).map(|r| r.map(Into::into)),
			Self::Data(t) => Pin::new(t).poll(cx).map(|r| r.map(Into::into)),
		}
	}
}
