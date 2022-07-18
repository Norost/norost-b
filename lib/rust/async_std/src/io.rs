pub use async_completion::*;
pub use rt::io::*;

use core::future::Future;

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

	fn write(&self, from: SeekFrom) -> Self::Future;
}
