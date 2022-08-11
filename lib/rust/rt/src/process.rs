use crate::{args, io, Handle, Object, RefObject};
use alloc::vec::Vec;

pub struct Process(Object);

impl Process {
	pub fn new<'a>(
		process_root: impl Into<RefObject<'a>>,
		binary_elf: impl Into<RefObject<'a>>,
		objects: impl Iterator<Item = (u32, impl Into<RefObject<'a>>)>,
		args: impl Iterator<Item = impl AsRef<[u8]>>,
		env: impl Iterator<Item = (impl AsRef<[u8]>, impl AsRef<[u8]>)>,
	) -> io::Result<Self> {
		Self::new_inner(
			process_root.into(),
			binary_elf.into(),
			objects.map(|(i, o)| (i, o.into())),
			args,
			env,
		)
	}

	pub fn new_by_name<'a>(
		name: impl AsRef<[u8]>,
		objects: impl Iterator<Item = (u32, impl Into<RefObject<'a>>)>,
		args: impl Iterator<Item = impl AsRef<[u8]>>,
		env: impl Iterator<Item = (impl AsRef<[u8]>, impl AsRef<[u8]>)>,
	) -> io::Result<Self> {
		Self::new(
			io::process_root().ok_or(io::Error::CantCreateObject)?,
			&io::file_root()
				.ok_or(io::Error::CantCreateObject)?
				.open(name.as_ref())?,
			objects.map(|(i, o)| (i, o.into())),
			args,
			env,
		)
	}

	fn new_inner<'a>(
		process_root: RefObject<'_>,
		binary_elf: RefObject<'_>,
		objects: impl Iterator<Item = (u32, RefObject<'a>)>,
		args: impl Iterator<Item = impl AsRef<[u8]>>,
		env: impl Iterator<Item = (impl AsRef<[u8]>, impl AsRef<[u8]>)>,
	) -> io::Result<Self> {
		let f = |n| u16::try_from(n).unwrap().to_ne_bytes();
		let proc = process_root.create(b"new")?;
		let mut stack = Vec::new();

		// binary
		proc.open(b"binary")?.share(&binary_elf)?;

		// objects
		{
			let proc_objects = proc.open(b"objects")?;
			let i = stack.len();
			let mut l = 0u32;
			stack.extend(0u32.to_ne_bytes());
			for (ty, h) in objects {
				let t = io::share(proc_objects.as_raw(), h.as_raw())?;
				let t = Handle::try_from(t).unwrap();
				stack.extend(ty.to_ne_bytes());
				stack.extend(t.to_ne_bytes());
				l += 1;
			}
			stack[i..i + 4].copy_from_slice(&l.to_ne_bytes());
			debug_assert_eq!(
				stack.len(),
				i + 4 + l as usize * 8,
				"stack has unexpected size"
			);
		}

		// args
		{
			let i = stack.len();
			let mut l = 0;
			stack.extend(f(0));
			for a in args {
				stack.extend(f(a.as_ref().len()));
				stack.extend(a.as_ref());
				l += 1;
			}
			stack[i..i + 2].copy_from_slice(&f(l));
		}

		// env
		{
			let i = stack.len();
			let mut l = 0;
			stack.extend(f(0));
			for (k, v) in env {
				stack.extend(f(k.as_ref().len()));
				stack.extend(k.as_ref());
				stack.extend(f(v.as_ref().len()));
				stack.extend(v.as_ref());
				l += 1;
			}
			stack[i..i + 2].copy_from_slice(&f(l));
		}

		proc.open(b"stack")?.write(&stack)?;

		proc.create(b"spawn").map(Self)
	}

	#[inline(always)]
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

	#[inline]
	pub fn default_handles<'a>() -> impl Iterator<Item = (u32, RefObject<'a>)> {
		Self::default_stdio_handles().chain(Self::default_root_handles())
	}

	#[inline]
	pub fn default_root_handles<'a>() -> impl Iterator<Item = (u32, RefObject<'a>)> {
		[
			(args::ID_FILE_ROOT, io::file_root()),
			(args::ID_NET_ROOT, io::net_root()),
			(args::ID_PROCESS_ROOT, io::process_root()),
		]
		.into_iter()
		.flat_map(|(ty, h)| h.map(|h| (ty, h)))
	}

	#[inline]
	pub fn default_stdio_handles<'a>() -> impl Iterator<Item = (u32, RefObject<'a>)> {
		[
			(args::ID_STDIN, io::stdin()),
			(args::ID_STDOUT, io::stdout()),
			(args::ID_STDERR, io::stderr()),
		]
		.into_iter()
		.flat_map(|(ty, h)| h.map(|h| (ty, h)))
	}
}

pub struct ExitStatus {
	pub code: u8,
}

pub struct Builder {
	builder: Object,
	objects_share: Option<Object>,
	objects: Vec<(u32, Handle)>,
	args: Vec<u8>,
	args_count: u16,
	env: Vec<u8>,
	env_count: u16,
}

impl Builder {
	pub fn new() -> io::Result<Self> {
		io::process_root()
			.ok_or(io::Error::CantCreateObject)?
			.create(b"new")
			.map(|builder| Self {
				builder,
				objects_share: None,
				objects: Default::default(),
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
		self.builder.open(b"binary")?.share(binary).map(|_| ())
	}

	pub fn add_object(&mut self, id: u32, object: &Object) -> io::Result<()> {
		if self.objects_share.is_none() {
			self.objects_share = Some(self.builder.open(b"objects")?);
		}
		let handle = self.objects_share.as_mut().unwrap().share(object)? as _;
		self.objects.push((id, handle));
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
		stack.extend_from_slice(&(self.objects.len() as u32).to_le_bytes());
		for (t, h) in self.objects {
			stack.extend_from_slice(&t.to_le_bytes());
			stack.extend_from_slice(&h.to_le_bytes());
		}

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
