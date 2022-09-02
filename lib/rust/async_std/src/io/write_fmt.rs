use {
	super::{Result, Write},
	alloc::vec::Vec,
	core::{
		future::Future,
		pin::Pin,
		task::{Context, Poll},
	},
};

pub struct WriteFmtFuture<T: Write<Vec<u8>> + ?Sized>
where
	T::Future: Unpin,
{
	pub(super) fut: Option<Result<T::Future>>,
}

impl<T: Write<Vec<u8>> + Unpin> Future for WriteFmtFuture<T>
where
	T::Future: Unpin,
{
	type Output = Result<()>;

	fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		match self.fut.take().expect("invalid future") {
			Ok(mut fut) => match Pin::new(&mut fut).poll(cx) {
				Poll::Ready((r, _)) => Poll::Ready(r.map(|_| ())),
				Poll::Pending => {
					self.fut = Some(Ok(fut));
					Poll::Pending
				}
			},
			Err(e) => Poll::Ready(Err(e)),
		}
	}
}
