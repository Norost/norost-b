use crate::{io, Handle, Object};
use alloc::vec::Vec;

pub struct Process(Object);

impl Process {
	pub fn as_object(&self) -> &Object {
		&self.0
	}

	pub fn into_object(self) -> Object {
		self.0
	}

	/// Wait until this process is destroyed.
	pub fn wait(self) -> io::Result<ExitStatus> {
		let mut v = [0; 2];
		self.0.get_meta(b"bin/wait".into(), (&mut v).into())?;
		Ok(ExitStatus { code: v[1] })
	}
}

pub struct ExitStatus {
	pub code: u8,
}

pub struct Builder {
	builder: Object,
	objects_share: Option<Object>,
	objects: Vec<u8>,
	objects_count: u16,
	args: Vec<u8>,
	args_count: u16,
	env: Vec<u8>,
	env_count: u16,
}

impl Builder {
	pub fn new() -> io::Result<Self> {
		let r = io::process_root().ok_or(io::Error::CantCreateObject)?;
		Self::new_with(&r)
	}

	pub fn new_with(process_root: &Object) -> io::Result<Self> {
		process_root.create(b"new").map(|builder| Self {
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

	pub fn set_binary_by_name(&mut self, name: &[u8]) -> io::Result<()> {
		self.set_binary(
			&io::file_root()
				.ok_or(io::Error::CantCreateObject)?
				.open(name)?,
		)
	}

	pub fn set_binary(&mut self, binary: &Object) -> io::Result<()> {
		self.set_binary_raw(binary.as_raw())
	}

	pub fn set_binary_raw(&mut self, binary: Handle) -> io::Result<()> {
		io::share(self.builder.open(b"binary")?.as_raw(), binary).map(|_| ())
	}

	pub fn add_object(&mut self, name: &[u8], object: &Object) -> io::Result<()> {
		self.add_object_raw(name, object.as_raw())
	}

	pub fn add_object_raw(&mut self, name: &[u8], object: Handle) -> io::Result<()> {
		if self.objects_share.is_none() {
			self.objects_share = Some(self.builder.open(b"objects")?);
		}
		let handle = io::share(self.objects_share.as_mut().unwrap().as_raw(), object)? as u32;
		inc(&mut self.objects_count)?;
		add_str(&mut self.objects, name)?;
		self.objects.extend_from_slice(&handle.to_le_bytes());
		Ok(())
	}

	pub fn add_default_objects(&mut self) -> io::Result<()> {
		self.add_default_stdio_objects()?;
		self.add_default_root_objects()
	}

	pub fn add_default_root_objects(&mut self) -> io::Result<()> {
		for (name, obj) in [
			("file", io::file_root()),
			("net", io::net_root()),
			("process", io::process_root()),
		] {
			if let Some(obj) = obj {
				self.add_object(name.as_bytes(), &obj)?;
			}
		}
		Ok(())
	}

	pub fn add_default_stdio_objects(&mut self) -> io::Result<()> {
		for (name, obj) in [
			("in", io::stdin()),
			("out", io::stdout()),
			("err", io::stderr()),
		] {
			if let Some(obj) = obj {
				self.add_object(name.as_bytes(), &obj)?;
			}
		}
		Ok(())
	}

	pub fn add_args<I>(&mut self, args: I) -> io::Result<()>
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

	pub fn spawn(self) -> io::Result<Process> {
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

		self.builder.open(b"stack")?.write(&stack)?;

		self.builder.create(b"spawn").map(Process)
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
