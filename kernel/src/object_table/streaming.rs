use super::*;
use crate::memory::frame::{self, PageFrame, PPN};
use crate::scheduler::process::Process;
use core::cell::{Cell, UnsafeCell};
use alloc::boxed::Box;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU16, Ordering};

pub struct StreamingTable {
	name: Box<str>,
	process: NonNull<Process>,
	commands: NonNull<CommandQueue>,
}

impl StreamingTable {
	pub fn new(name: Box<str>, process: NonNull<Process>) -> (Self, Box<dyn Interface>) {
		use crate::arch::r#virtual::phys_to_virt;
		use core::num::NonZeroUsize;
		let commands = frame::allocate_contiguous(NonZeroUsize::new(1).unwrap()).unwrap();
		let commands = unsafe { phys_to_virt(commands.as_phys().try_into().unwrap()).cast() };
		let commands = NonNull::new(commands).unwrap();

		(Self { name, process, commands }, Box::new(StreamTableInterface { commands }))
	}
}

impl Table for StreamingTable {
	fn name(&self) -> &str {
		&self.name
	}

	fn query(&self, name: Option<&str>, tags: &[&str]) -> Box<dyn Query> {
		todo!()
	}

	fn get(&self, id: Id) -> Option<Object> {
		let cmds = unsafe { self.commands.as_ref() };

		// Send command
		let cmd = Command::open(42, id.into());
		cmds.push_command(cmd).unwrap();

		// Wait for response
		let rsp = loop {
			dbg!(self.commands);
			dbg!(cmds.commands_tail.load(Ordering::Relaxed), cmds.commands_head.load(Ordering::Relaxed));
			if let Some(rsp) = cmds.pop_response() {
				break rsp;
			}
			unsafe { asm!("int 61") }; // Fake timer interrupt.
			dbg!();
		};
		let body = unsafe { rsp.body.open };
		assert_eq!(rsp.cmd_id, 42);

		use crate::memory::r#virtual::phys_to_virt;
		let f = |p: Option<NonNull<_>>| unsafe {
			self.process.as_ref().get_physical_address(p.unwrap().cast()).unwrap().0
		};
		let g = |p: usize| unsafe {
			NonNull::new(phys_to_virt(p.try_into().unwrap())).unwrap()
		};
		let write_ptr = g(f(body.write_ptr));
		let read_ptr  = g(f(body.read_ptr ));
		let write_mask = (1 << body.write_p2size) - 1;
		let read_mask  = (1 << body.read_p2size ) - 1;

		let write_queue = FixedQueue {
			buffer_ptr: write_ptr,
			mask: write_mask,
			head: 0,
			tail: 0,
		};
		let read_queue = FixedQueue {
			buffer_ptr: read_ptr,
			mask: read_mask,
			head: 0,
			tail: 0,
		};

		Some(Object {
			id,
			name: "".into(),
			tags: [].into(),
			interface: Box::new(StreamObject {
				read_queue: SpinLock::new(read_queue),
				write_queue: SpinLock::new(write_queue),
				commands: self.commands,
			}),
		})
	}

	fn create(&self, name: &str, tags: &[&str]) -> Result<Object, CreateObjectError> {
		todo!()
	}
}

#[derive(Clone)]
struct StreamTableInterface {
	commands: NonNull<CommandQueue>,
}

impl MemoryObject for StreamTableInterface {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		use crate::arch::r#virtual::virt_to_phys;
		[PageFrame { base: unsafe { PPN::from_ptr(self.commands.as_ptr().cast()) }, p2size: 0 }].into()
	}
}

impl Interface for StreamTableInterface {
	fn memory_object(&self, _: u64) -> Option<Box<dyn MemoryObject>> {
		Some(Box::new(self.clone()))
	}
}

#[repr(C)]
struct CommandQueue {
	commands: [UnsafeCell<Command>; 64],
	responses: [UnsafeCell<Response>; 64],
	commands_head: AtomicU16,
	commands_tail: AtomicU16,
	responses_head: AtomicU16,
	responses_tail: AtomicU16,
}

impl CommandQueue {
	fn push_command(&self, cmd: Command) -> Result<(), ()> {
		let h = self.commands_head.load(Ordering::Relaxed);
		let t = self.commands_tail.load(Ordering::Relaxed);
		let l = self.commands.len();
		if h == t.wrapping_add(l.try_into().unwrap()) {
			return Err(());
		}
		unsafe {
			self.commands[usize::from(h) % l].get().write(cmd);
		}
		let h = h.wrapping_add(1);
		self.commands_head.store(h, Ordering::Release);
		Ok(())
	}

	fn pop_response(&self) -> Option<Response> {
		let t = self.responses_tail.load(Ordering::Acquire);
		let h = self.responses_head.load(Ordering::Relaxed);
		let l = self.responses.len();
		if t == h {
			return None;
		}
		let rsp = unsafe {
			self.responses[usize::from(t) % l].get().read()
		};
		let t = t.wrapping_add(1);
		self.responses_tail.store(t, Ordering::Relaxed);
		Some(rsp)
	}
}

#[repr(C)]
struct Command {
	op: u8,
	_padding: u8,
	cmd_id: u16,
	body: CommandBody,
}

impl Command {
	fn open(cmd_id: u16, id: u64) -> Self {
		Self {
			op: 0,
			_padding: 0,
			cmd_id,
			body: unsafe { CommandBody { open: CommandOpen { id } } },
		}
	}

	fn write(cmd_id: u16, handle: u32, count: usize) -> Self {
		Self {
			op: 2,
			_padding: 0,
			cmd_id,
			body: unsafe { CommandBody { write: CommandWrite { handle, count } } },
		}
	}
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
struct Response {
	status: u8,
	_padding: u8,
	cmd_id: u16,
	body: ResponseBody,
}

union ResponseBody {
	open: ResponseOpen,
	write: ResponseWrite,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ResponseOpen {
	write_ptr: Option<NonNull<u8>>,
	read_ptr: Option<NonNull<u8>>,
	read_p2size: u8,
	write_p2size: u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ResponseWrite {
	size: usize,
}

struct FixedQueue {
	buffer_ptr: NonNull<u8>,
	mask: u16,
	head: u16,
	tail: u16,
}

impl FixedQueue {
	fn enqueue(&mut self, mut data: &[u8]) -> usize {
		let l = self.mask.wrapping_add(1);
		let c = data.len().min(self.tail.wrapping_add(l).wrapping_sub(self.head).into());
		dbg!(self.buffer_ptr);
		while self.head != self.tail.wrapping_add(l) && data.len() > 0 {
			unsafe {
				dbg!(self.head & self.mask);
				*self.buffer_ptr.as_ptr().add(usize::from(self.head & self.mask)) = data[0];
			}
			self.head = self.head.wrapping_add(1);
			data = &data[1..];
		}
		c
	}

	fn dequeue(&mut self, mut data: &mut [u8]) -> usize {
		let c = data.len().min(self.head.wrapping_sub(self.tail).into());
		while self.tail != self.head && data.len() > 0 {
			unsafe {
				data[0] = *self.buffer_ptr.as_ptr().add(usize::from(self.head & self.mask));
			}
			self.tail = self.tail.wrapping_add(1);
			data = &mut data[1..];
		}
		c
	}
}

struct StreamObject {
	read_queue: SpinLock<FixedQueue>,
	write_queue: SpinLock<FixedQueue>,
	commands: NonNull<CommandQueue>,
}

impl Interface for StreamObject {
	fn read(&self, _: u64, data: &mut [u8]) -> Result<usize, ()> {
		let cmd = unsafe { self.commands.as_ref() };
		let r = self.read_queue.lock().enqueue(data);
		cmd.push_command(todo!());
		Ok(r)
	}

	fn write(&self, _: u64, data: &[u8]) -> Result<usize, ()> {
		let cmd = unsafe { self.commands.as_ref() };
		let r = self.write_queue.lock().enqueue(data);
		if r > 0 {
			cmd.push_command(Command::write(14, u32::MAX, r));
		}

		// Wait for response
		let rsp = loop {
			if let Some(rsp) = cmd.pop_response() {
				break rsp;
			}
			unsafe { asm!("int 61") }; // Fake timer interrupt.
			dbg!();
		};
		let body = unsafe { rsp.body.write };
		assert_eq!(rsp.cmd_id, 14);

		dbg!(Ok(body.size))
	}
}
