use {
	crate::{
		io::{self, Buf, BufMut},
		queue,
	},
	core::{
		marker::PhantomData,
		mem::{self, ManuallyDrop},
		ops::Deref,
	},
};

#[repr(transparent)]
pub struct AsyncObject(rt::Handle);

impl AsyncObject {
	pub fn into_raw(self) -> rt::Handle {
		ManuallyDrop::new(self).0
	}

	pub fn as_raw(&self) -> rt::Handle {
		self.0
	}

	pub fn from_raw(handle: rt::Handle) -> Self {
		Self(handle)
	}

	pub async fn open<B: Buf>(&self, path: B) -> (io::Result<Self>, B) {
		let (res, b) = queue::submit(|q, b| q.submit_open(self.0, b), path).await;
		(res.map(Self), b)
	}

	pub async fn create<B: Buf>(&self, path: B) -> (io::Result<Self>, B) {
		let (res, b) = queue::submit(|q, b| q.submit_create(self.0, b), path).await;
		(res.map(Self), b)
	}

	pub async fn get_meta<B, Bm>(&self, property: B, value: Bm) -> (io::Result<u8>, B, Bm)
	where
		B: Buf,
		Bm: BufMut,
	{
		let (res, b, bm) =
			queue::submit2(|q, b, bm| q.submit_get_meta(self.0, b, bm), property, value).await;
		(res, b, bm)
	}

	pub async fn share(&self, object: AsyncObject) -> (io::Result<u64>, AsyncObject) {
		(self.share_raw(object.0).await, object)
	}

	pub async fn share_raw(&self, handle: rt::Handle) -> io::Result<u64> {
		queue::submit(|q, ()| q.submit_share(self.0, handle), ()).await
	}
}

impl From<rt::Object> for AsyncObject {
	fn from(obj: rt::Object) -> Self {
		Self(obj.into_raw())
	}
}

impl From<AsyncObject> for rt::Object {
	fn from(obj: AsyncObject) -> Self {
		Self::from_raw(ManuallyDrop::new(obj).0)
	}
}

impl<'a> From<&'a AsyncObject> for rt::RefObject<'a> {
	fn from(obj: &'a AsyncObject) -> Self {
		Self::from_raw(obj.0)
	}
}

impl<B: io::BufMut> io::Read<B> for AsyncObject {
	type Future = io_queue_rt::Read<'static, B>;

	fn read(&self, buf: B) -> Self::Future {
		queue::submit(|q, b| q.submit_read(self.0, b), buf)
	}
}

impl<B: io::Buf> io::Write<B> for AsyncObject {
	type Future = io_queue_rt::Write<'static, B>;

	fn write(&self, buf: B) -> Self::Future {
		queue::submit(|q, b| q.submit_write(self.0, b), buf)
	}
}

macro_rules! impl_wrap {
	($ty:ident read) => {
		impl<B: crate::io::BufMut> crate::io::Read<B> for $ty {
			type Future = <$crate::object::AsyncObject as crate::io::Read<B>>::Future;

			fn read(&self, buf: B) -> Self::Future {
				self.0.read(buf)
			}
		}
	};
	($ty:ident write) => {
		impl<B: crate::io::Buf> crate::io::Write<B> for $ty {
			type Future = <$crate::object::AsyncObject as crate::io::Write<B>>::Future;

			fn write(&self, buf: B) -> Self::Future {
				self.0.write(buf)
			}
		}
	};
}

impl Drop for AsyncObject {
	fn drop(&mut self) {
		let _ = queue::get()
			.submit_close(self.0)
			.expect("todo: wrapper that blocks");
	}
}

/// An object by "reference" but with less indirection.
#[derive(Clone, Copy)]
pub struct RefAsyncObject<'a> {
	handle: rt::Handle,
	_marker: PhantomData<&'a AsyncObject>,
}

impl<'a> RefAsyncObject<'a> {
	pub fn as_raw(&self) -> rt::Handle {
		self.0
	}

	pub fn from_raw(handle: rt::Handle) -> Self {
		Self { handle, _marker: PhantomData }
	}
}

impl<'a> From<&'a rt::Object> for RefAsyncObject<'a> {
	fn from(obj: &'a rt::Object) -> Self {
		Self::from(rt::RefObject::from(obj))
	}
}

impl<'a> From<&'a AsyncObject> for RefAsyncObject<'a> {
	fn from(obj: &'a AsyncObject) -> Self {
		Self { handle: obj.0, _marker: PhantomData }
	}
}

impl<'a> From<rt::RefObject<'a>> for RefAsyncObject<'a> {
	fn from(obj: rt::RefObject<'a>) -> Self {
		Self { handle: obj.as_raw(), _marker: PhantomData }
	}
}

impl<'a> From<RefAsyncObject<'a>> for rt::RefObject<'a> {
	fn from(obj: RefAsyncObject<'a>) -> Self {
		Self::from_raw(obj.handle)
	}
}

impl<'a> Deref for RefAsyncObject<'a> {
	type Target = AsyncObject;

	fn deref(&self) -> &Self::Target {
		// SAFETY: Object is a simple wrapper around the handle.
		unsafe { mem::transmute(&self.handle) }
	}
}

pub fn file_root() -> RefAsyncObject<'static> {
	RefAsyncObject::from(io::file_root().expect("no file root"))
}

pub fn process_root() -> RefAsyncObject<'static> {
	RefAsyncObject::from(io::process_root().expect("no process root"))
}
