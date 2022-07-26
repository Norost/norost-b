use crate::queue;
use core::{
	future::Future,
	pin::Pin,
	task::{Context, Poll},
	time::Duration,
};

pub fn block_on<R>(fut: impl Future<Output = R>) -> R {
	futures_lite::pin!(fut);
	let mut cx = Context::from_waker(futures_task::noop_waker_ref());
	loop {
		if let Poll::Ready(r) = Pin::new(&mut fut).poll(&mut cx) {
			return r;
		}
		queue::wait(Duration::MAX);
	}
}
