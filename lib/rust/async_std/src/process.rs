use {
	crate::{
		io::{self, Write},
		object::file_root,
		AsyncObject, RefAsyncObject,
	},
	alloc::vec::Vec,
};

pub use rt::process::ExitStatus;

pub struct Process(AsyncObject);

impl Process {
	pub fn as_object(&self) -> &AsyncObject {
		&self.0
	}

	pub fn into_object(self) -> AsyncObject {
		self.0
	}

	/// Wait until this process is destroyed.
	pub async fn wait(&self) -> io::Result<ExitStatus> {
		let (res, _, v) = self.0.get_meta(b"bin/wait", Vec::with_capacity(2)).await;
		res.map(|_| ExitStatus { code: v[1] })
	}
}

pub struct Builder {
	builder: AsyncObject,
	objects_share: Option<AsyncObject>,
	objects: Vec<u8>,
	objects_count: u16,
	args: Vec<u8>,
	args_count: u16,
	env: Vec<u8>,
	env_count: u16,
}

impl Builder {
	pub async fn new() -> io::Result<Self> {
		let r = io::process_root().ok_or(io::Error::CantCreateObject)?;
		Self::new_with(&RefAsyncObject::from_raw(r.as_raw())).await
	}

	pub async fn new_with(process_root: &AsyncObject) -> io::Result<Self> {
		process_root.create(b"new").await.0.map(|builder| Self {
			builder,
			objects_share: None,
			objects: Default::default(),
			objects_count: Default::default(),
			args: Default::default(),
			args_count: Default::default(),
			env: Default::default(),
			env_count: Default::default(),
		})
	}

	pub async fn set_binary_by_name<B: io::Buf>(&mut self, name: B) -> (io::Result<()>, B) {
		match file_root().open(name).await {
			(Ok(obj), name) => (self.set_binary(obj).await.0, name),
			(Err(e), name) => (Err(e), name),
		}
	}

	pub async fn set_binary(&mut self, binary: AsyncObject) -> (io::Result<()>, AsyncObject) {
		match self.builder.open(b"binary").await.0 {
			Ok(b) => {
				let (res, bin) = b.share(binary).await;
				(res.map(|_| ()), bin)
			}
			Err(e) => (Err(e), binary),
		}
	}

	pub async fn add_object(
		&mut self,
		name: &[u8],
		object: AsyncObject,
	) -> (io::Result<()>, AsyncObject) {
		(self.add_object_raw(name, object.as_raw()).await, object)
	}

	pub async fn add_object_raw(&mut self, name: &[u8], handle: rt::Handle) -> io::Result<()> {
		if self.objects_share.is_none() {
			self.objects_share = Some(self.builder.open(b"objects").await.0?);
		}
		let handle = self
			.objects_share
			.as_mut()
			.unwrap()
			.share_raw(handle)
			.await? as u32;
		inc(&mut self.objects_count)?;
		add_str(&mut self.objects, name)?;
		self.objects.extend_from_slice(&handle.to_le_bytes());
		Ok(())
	}

	pub async fn add_default_objects(&mut self) -> io::Result<()> {
		self.add_default_stdio_objects().await?;
		self.add_default_root_objects().await
	}

	pub async fn add_default_root_objects(&mut self) -> io::Result<()> {
		for (name, obj) in [
			("file", io::file_root()),
			("net", io::net_root()),
			("process", io::process_root()),
		] {
			if let Some(obj) = obj {
				self.add_object_raw(name.as_bytes(), obj.as_raw()).await?;
			}
		}
		Ok(())
	}

	pub async fn add_default_stdio_objects(&mut self) -> io::Result<()> {
		for (name, obj) in [
			("in", rt::io::stdin()),
			("out", rt::io::stdout()),
			("err", rt::io::stderr()),
		] {
			if let Some(obj) = obj {
				self.add_object_raw(name.as_bytes(), obj.as_raw()).await?;
			}
		}
		Ok(())
	}

	pub async fn add_args<I>(&mut self, args: I) -> io::Result<()>
	where
		I: IntoIterator,
		I::IntoIter: ExactSizeIterator,
		<I::IntoIter as Iterator>::Item: AsRef<[u8]>,
	{
		let args = args.into_iter();
		if let Some(n) = usize::from(self.args_count)
			.checked_add(args.len())
			.and_then(|n| u16::try_from(n).ok())
		{
			for a in args {
				let l = u16::try_from(a.as_ref().len()).map_err(|_| io::Error::CantCreateObject)?;
				self.args.extend_from_slice(&l.to_le_bytes());
				self.args.extend_from_slice(a.as_ref());
			}
			self.args_count = n;
			Ok(())
		} else {
			Err(io::Error::CantCreateObject)
		}
	}

	pub async fn spawn(self) -> io::Result<Process> {
		let mut stack = Vec::new();

		// objects
		stack.extend_from_slice(&self.objects_count.to_le_bytes());
		stack.extend_from_slice(&self.objects);

		// args
		stack.extend_from_slice(&self.args_count.to_le_bytes());
		stack.extend_from_slice(&self.args);

		// env
		stack.extend_from_slice(&self.env_count.to_le_bytes());
		stack.extend_from_slice(&self.env);

		self.builder.open(b"stack").await.0?.write(stack).await.0?;

		self.builder.create(b"spawn").await.0.map(Process)
	}
}

fn add_str(buf: &mut Vec<u8>, s: &[u8]) -> io::Result<()> {
	u16::try_from(s.len())
		.map(|l| {
			buf.extend_from_slice(&l.to_le_bytes());
			buf.extend_from_slice(s);
		})
		.map_err(|_| io::Error::InvalidData)
}

fn inc(counter: &mut u16) -> io::Result<()> {
	counter
		.checked_add(1)
		.ok_or(io::Error::CantCreateObject)
		.map(|c| *counter = c)
}
