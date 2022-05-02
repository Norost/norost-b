#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use crate::{args, io, Error, Handle, Object};
use alloc::vec::Vec;

pub struct Process(Object);

impl Process {
	pub fn new<'a>(
		binary_elf: &Object,
		objects: impl Iterator<Item = (u32, Handle)>,
		args: impl Iterator<Item = &'a [u8]> + ExactSizeIterator,
		env: impl Iterator<Item = (&'a [u8], &'a [u8])> + ExactSizeIterator,
	) -> io::Result<Self> {
		let f = |n| u16::try_from(n).unwrap().to_ne_bytes();
		let proc = io::process_root()
			.ok_or(Error::InvalidOperation)?
			.create(b"process/new")?;

		// binary
		proc.open(b"binary")?.share(&binary_elf)?;
		let mut stack = Vec::new();

		// objects
		let proc_objects = proc.open(b"objects")?;
		for (ty, h) in objects {
			let t = u32::try_from(io::share(proc_objects.as_raw(), h)?).unwrap();
			stack.extend(ty.to_ne_bytes());
			stack.extend(t.to_ne_bytes());
		}

		// args
		stack.extend(f(args.len()));
		for a in args {
			stack.extend(f(a.len()));
			stack.extend(a);
		}

		// env
		stack.extend(f(env.len()));
		for (k, v) in env {
			stack.extend(f(k.len()));
			stack.extend(k);
			stack.extend(f(v.len()));
			stack.extend(v);
		}

		proc.open(b"stack")?.write(&stack)?;

		proc.create(b"spawn").map(Self)
	}

	#[inline(always)]
	pub fn as_object(&self) -> &Object {
		&self.0
	}

	#[inline]
	pub fn default_handles() -> impl Iterator<Item = (u32, Handle)> {
		Self::default_stdio_handles().chain(Self::default_root_handles())
	}

	#[inline]
	pub fn default_root_handles() -> impl Iterator<Item = (u32, Handle)> {
		[
			(args::ID_FILE_ROOT, io::file_root()),
			(args::ID_NET_ROOT, io::net_root()),
			(args::ID_PROCESS_ROOT, io::process_root()),
		]
		.into_iter()
		.flat_map(|(ty, o)| o.map(|o| (ty, o.as_raw())))
	}

	#[inline]
	pub fn default_stdio_handles() -> impl Iterator<Item = (u32, Handle)> {
		[
			(args::ID_STDIN, io::stdin()),
			(args::ID_STDOUT, io::stdout()),
			(args::ID_STDERR, io::stderr()),
		]
		.into_iter()
		.flat_map(|(ty, o)| o.map(|o| (ty, o.as_raw())))
	}
}
