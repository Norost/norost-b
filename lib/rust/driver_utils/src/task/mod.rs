pub mod waker;

use core::{
	future::Future,
	pin::Pin,
	task::{Context, Poll},
};

/// A convienence method for polling futures with less boilerplate.
pub fn poll<T, F>(task: &mut F) -> Option<T>
where
	F: Future<Output = T> + Unpin,
{
	let wk = waker::dummy();
	let mut cx = Context::from_waker(&wk);
	match Pin::new(task).poll(&mut cx) {
		Poll::Ready(t) => Some(t),
		Poll::Pending => None,
	}
}
