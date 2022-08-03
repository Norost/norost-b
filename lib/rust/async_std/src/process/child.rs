use crate::{io, AsyncObject};
use alloc::vec::Vec;

pub struct ChildStdin(pub(super) AsyncObject);
pub struct ChildStdout(pub(super) AsyncObject);
pub struct ChildStderr(pub(super) AsyncObject);

impl_wrap!(ChildStdin write);
impl_wrap!(ChildStdout read);
impl_wrap!(ChildStderr read);

pub struct Child {
	pub(super) process: AsyncObject,
	pub stdin: Option<ChildStdin>,
	pub stdout: Option<ChildStdout>,
	pub stderr: Option<ChildStderr>,
}

impl Child {
	pub async fn wait(&self) -> io::Result<ExitStatus> {
		let v = Vec::with_capacity(2);
		let (res, _, v) = self.process.get_meta(b"bin/wait", v).await;
		res.map(|_| ExitStatus { code: v[1] })
	}
}

pub struct ExitStatus {
	code: u8,
}

impl ExitStatus {
	pub fn code(&self) -> Option<i32> {
		Some(self.code.into())
	}
}
