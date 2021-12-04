use super::*;
use core::cell::{Cell, UnsafeCell};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU16, Ordering};
use crate::Page;

#[repr(C)]
pub struct CommandQueue {
	commands: [UnsafeCell<CommandFFI>; 64],
	responses: [UnsafeCell<Response>; 64],
	commands_head: AtomicU16,
	commands_tail: AtomicU16,
	responses_head: AtomicU16,
	responses_tail: AtomicU16,
}

impl CommandQueue {
	pub fn push_response(&self, rsp: Response) -> Result<(), ()> {
		let h = self.responses_head.load(Ordering::Relaxed);
		let t = self.responses_tail.load(Ordering::Relaxed);
		let l = self.responses.len();
		if h == t.wrapping_add(l.try_into().unwrap()) {
			return Err(());
		}
		unsafe {
			self.responses[usize::from(h) % l].get().write(rsp);
		}
		let h = h.wrapping_add(1);
		self.responses_head.store(h, Ordering::Release);
		Ok(())
	}

	pub fn pop_command(&self) -> Option<Command> {
		let t = self.commands_tail.load(Ordering::Acquire);
		let h = self.commands_head.load(Ordering::Relaxed);
		syslog!("tail {}     head {}", t, h);
		let l = self.commands.len();
		if t == h {
			return None;
		}
		let rsp = unsafe {
			self.commands[usize::from(t) % l].get().read()
		};
		let t = t.wrapping_add(1);
		self.commands_tail.store(t, Ordering::Relaxed);
		Some(rsp.into())
	}
}

pub enum Command {
	Open { id: crate::syscall::Id, cmd_id: u16 },
	Write { handle: u32, count: usize, cmd_id: u16 },
}

impl From<CommandFFI> for Command {
	fn from(c: CommandFFI) -> Self {
		unsafe {
			let cmd_id = c.cmd_id;
			match c.op {
				0 => Self::Open { id: crate::syscall::Id(c.body.open.id), cmd_id },
				2 => Self::Write { handle: c.body.write.handle, count: c.body.write.count, cmd_id },
				_ => unreachable!(),
			}
		}
	}
}

#[repr(C)]
struct CommandFFI {
	op: u8,
	_padding: u8,
	cmd_id: u16,
	body: CommandBody,
}

union CommandBody {
	open: CommandOpen,
	write: CommandWrite,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CommandOpen {
	id: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct CommandWrite {
	handle: u32,
	count: usize,
}

#[repr(C)]
pub struct Response {
	status: u8,
	_padding: u8,
	cmd_id: u16,
	body: ResponseBody,
}

impl Response {
	pub fn open(c: &Command, write_ptr: NonNull<Page>, write_p2size: u8, read_ptr: NonNull<Page>, read_p2size: u8) -> Result<Self, ()> {
		let (body, cmd_id) = match c {
			Command::Open { cmd_id, .. } => (ResponseBody { open: ResponseOpen { write_ptr: Some(write_ptr), read_ptr: Some(read_ptr), write_p2size, read_p2size } }, *cmd_id),
			_ => Err(())?,
		};
		Ok(Self { status: 0, _padding: 0, cmd_id, body })
	}

	pub fn write(c: &Command, size: usize) -> Result<Self, ()> {
		let (body, cmd_id) = match c {
			Command::Write { cmd_id, count, .. } => (ResponseBody { write: ResponseWrite { size } }, *cmd_id),
			_ => Err(())?,
		};
		Ok(Self { status: 0, _padding: 0, cmd_id, body })
	}
}

union ResponseBody {
	open: ResponseOpen,
	write: ResponseWrite,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ResponseOpen {
	write_ptr: Option<NonNull<Page>>,
	read_ptr: Option<NonNull<Page>>,
	read_p2size: u8,
	write_p2size: u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ResponseWrite {
	size: usize,
}
