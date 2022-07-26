use crate::{io, AsyncObject};
use core::future::Future;

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
	pub fn wait(&self) -> impl Future<Output = io::Result<ExitStatus>> {
		//async move { todo!() }
		core::future::pending()
	}
}

pub struct ExitStatus {}

impl ExitStatus {
	pub fn code(&self) -> Option<i32> {
		None
	}
}
