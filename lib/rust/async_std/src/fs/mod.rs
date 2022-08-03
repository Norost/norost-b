use crate::{
	io::{Buf, Read},
	object::file_root,
	AsyncObject,
};
use alloc::vec::Vec;
use rt::io;

pub struct File(AsyncObject);

impl_wrap!(File read);
impl_wrap!(File write);

impl File {
	pub async fn open<B: Buf>(&self, path: B) -> (io::Result<File>, B) {
		let (f, path) = file_root().open(path).await;
		(f.map(File), path)
	}

	pub async fn create<B: Buf>(&self, path: B) -> (io::Result<File>, B) {
		let (f, path) = file_root().create(path).await;
		(f.map(File), path)
	}
}

pub async fn read<B: Buf>(path: B) -> (io::Result<Vec<u8>>, B) {
	let (f, path) = file_root().open(path).await;
	let f = match f {
		Ok(f) => f,
		Err(e) => return (Err(e), path),
	};
	let mut v = Vec::new();
	loop {
		v.reserve(2048);
		let l = v.len();
		match f.read(v.slice(l..)).await {
			(Ok(0), nv) => break (Ok(nv.into_inner()), path),
			(Ok(_), nv) => v = nv.into_inner(),
			(Err(e), _) => break (Err(e), path),
		}
	}
}
