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
