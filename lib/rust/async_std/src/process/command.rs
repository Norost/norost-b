use super::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio, StdioTy};
use crate::{
	io,
	object::{file_root, RefAsyncObject},
	AsyncObject,
};
use alloc::{boxed::Box, vec::Vec};
use core::mem;

pub struct Command {
	program: Box<[u8]>,
	args: Vec<Box<[u8]>>,
	env: Vec<(Box<[u8]>, Box<[u8]>)>,
	stdin: Stdio,
	stdout: Stdio,
	stderr: Stdio,
}

// Make everything async just in case we need it in the future (hah)
impl Command {
	pub async fn new(program: impl AsRef<[u8]>) -> Self {
		Self {
			program: program.as_ref().into(),
			args: Default::default(),
			env: Default::default(),
			stdin: Stdio(StdioTy::Inherit),
			stdout: Stdio(StdioTy::Inherit),
			stderr: Stdio(StdioTy::Inherit),
		}
	}

	pub async fn args(&mut self, args: impl IntoIterator<Item = impl AsRef<[u8]>>) -> &mut Self {
		self.args
			.extend(args.into_iter().map(|e| e.as_ref().into()));
		self
	}

	pub async fn arg(&mut self, arg: impl AsRef<[u8]>) -> &mut Self {
		self.args.push(arg.as_ref().into());
		self
	}

	pub async fn stdin(&mut self, stdin: impl Into<Stdio>) -> &mut Self {
		self.stdin = stdin.into();
		self
	}

	pub async fn stdout(&mut self, stdout: impl Into<Stdio>) -> &mut Self {
		self.stdout = stdout.into();
		self
	}

	pub async fn stderr(&mut self, stderr: impl Into<Stdio>) -> &mut Self {
		self.stderr = stderr.into();
		self
	}

	pub async fn spawn(&mut self) -> io::Result<Child> {
		let mut stdin = None;
		let mut stdout = None;
		let mut stderr = None;

		fn io<'a>(
			n: u32,
			io: &mut Stdio,
			og: Option<rt::RefObject<'a>>,
			sto: &'a mut Option<(rt::Object, rt::Object)>,
			dir: bool,
		) -> io::Result<impl Iterator<Item = (u32, rt::RefObject<'a>)>> {
			let io = mem::replace(&mut io.0, StdioTy::Inherit);
			Ok(match io {
				StdioTy::Null => None,
				StdioTy::Inherit => og,
				StdioTy::Piped => {
					let (wr, rd) = rt::Object::new(rt::NewObject::Pipe)?;
					let (a, b) = if dir { (rd, wr) } else { (wr, rd) };
					*sto = Some((a, b));
					sto.as_ref().map(|(_, o)| rt::RefObject::from(o))
				}
			}
			.into_iter()
			.map(move |o| (n, o)))
		}

		let (res, p) = file_root().open(mem::take(&mut self.program)).await;
		self.program = p;
		use rt::{args::*, io};
		// FIXME blocking, technically
		let proc = rt::process::Process::new(
			rt::io::process_root().unwrap(),
			&res?,
			io(ID_STDIN, &mut self.stdin, io::stdin(), &mut stdin, false)?
				.chain(io(
					ID_STDOUT,
					&mut self.stdout,
					io::stdout(),
					&mut stdout,
					true,
				)?)
				.chain(io(
					ID_STDERR,
					&mut self.stderr,
					io::stderr(),
					&mut stderr,
					true,
				)?)
				.chain(rt::process::Process::default_root_handles()),
			self.args.iter(),
			self.env.iter().map(|(a, b)| (a, b)),
		)?;
		Ok(Child {
			process: proc.into_object().into(),
			stdin: stdin.map(|(o, _)| ChildStdin(o.into())),
			stdout: stdout.map(|(o, _)| ChildStdout(o.into())),
			stderr: stderr.map(|(o, _)| ChildStderr(o.into())),
		})
	}
}
