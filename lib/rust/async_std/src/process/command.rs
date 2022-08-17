use super::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio, StdioTy};
use crate::{io, object::file_root};
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
		let (res, p) = file_root().open(mem::take(&mut self.program)).await;
		self.program = p;
		// FIXME blocking, technically
		let mut b = rt::process::Builder::new()?;
		b.set_binary_raw(res?.as_raw())?;
		let mut io = |n: &[u8], io: &mut Stdio, og: Option<rt::RefObject<'static>>| {
			let io = mem::replace(&mut io.0, StdioTy::Inherit);
			match io {
				StdioTy::Null => Ok::<_, rt::Error>(None),
				StdioTy::Inherit => {
					if let Some(og) = og {
						b.add_object(n, &og)?;
					}
					Ok(None)
				}
				StdioTy::Piped => {
					let (wr, rd) = rt::Object::new(rt::NewObject::Pipe)?;
					let (proc, slf) = if n == b"in" { (rd, wr) } else { (wr, rd) };
					b.add_object(n, &proc)?;
					Ok(Some(slf))
				}
			}
		};
		let stdin = io(b"in", &mut self.stdin, rt::io::stdin())?;
		let stdout = io(b"out", &mut self.stdout, rt::io::stdout())?;
		let stderr = io(b"err", &mut self.stderr, rt::io::stderr())?;
		b.add_default_root_objects()?;
		let proc = b.spawn()?;
		Ok(Child {
			process: proc.into_object().into(),
			stdin: stdin.map(|o| ChildStdin(o.into())),
			stdout: stdout.map(|o| ChildStdout(o.into())),
			stderr: stderr.map(|o| ChildStderr(o.into())),
		})
	}
}
