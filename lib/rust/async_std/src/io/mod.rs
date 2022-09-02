mod write_fmt;

pub use {async_completion::*, rt::io::*, write_fmt::WriteFmtFuture};

use {
	crate::object::RefAsyncObject,
	alloc::{string::String, vec::Vec},
	core::{fmt, future::Future},
};

pub trait Read<B: BufMut> {
	type Future: Future<Output = (Result<usize>, B)>;

	fn read(&self, buf: B) -> Self::Future;
}

pub trait Write<B: Buf> {
	type Future: Future<Output = (Result<usize>, B)>;

	fn write(&self, buf: B) -> Self::Future;
}

pub trait Seek {
	type Future: Future<Output = Result<u64>>;

	fn seek(&self, from: SeekFrom) -> Self::Future;
}

pub trait WriteFmt: Write<Vec<u8>>
where
	Self::Future: Unpin,
{
	fn write_fmt(&self, args: fmt::Arguments<'_>) -> WriteFmtFuture<Self>;
}

impl<T: Write<Vec<u8>> + Unpin> WriteFmt for T
where
	Self::Future: Unpin,
{
	fn write_fmt(&self, args: fmt::Arguments<'_>) -> WriteFmtFuture<Self> {
		// Really inefficient but there isn't much we can do as all the necessary fields are
		// private.
		let mut string = String::new();
		let res = core::fmt::write(&mut string, args)
			.map(|_| string.into_bytes())
			.map_err(|_| rt::Error::InvalidData);
		WriteFmtFuture { fut: Some(res.map(|b| self.write(b))) }
	}
}

pub struct Stdin(RefAsyncObject<'static>);

pub struct Stdout(RefAsyncObject<'static>);

pub struct Stderr(RefAsyncObject<'static>);

impl_wrap!(Stdin read);
impl_wrap!(Stdout write);
impl_wrap!(Stderr write);

fn stdout() -> Stdout {
	Stdout(rt::io::stdout().expect("no stdout").into())
}

pub fn stderr() -> Stderr {
	Stderr(rt::io::stderr().expect("no stderr").into())
}

#[doc(hidden)]
pub async fn __print(args: fmt::Arguments<'_>) {
	stdout().write_fmt(args).await.unwrap();
}

#[doc(hidden)]
pub async fn __eprint(args: fmt::Arguments<'_>) {
	stderr().write_fmt(args).await.unwrap();
}
