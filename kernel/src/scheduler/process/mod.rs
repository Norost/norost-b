mod elf;

use crate::arch;
use crate::ipc::queue::{ClientQueue, NewClientQueueError};
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, Mappable, RWX};
use crate::memory::Page;

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	//in_ports: Vec<Option<InPort>>,:
	//out_ports: Vec<Option<OutPort>>,
	//named_ports: Box<[ReverseNamedPort]>,
	thread: Option<super::Thread>,
	//threads: Vec<NonNull<Thread>>,
	client_queue: Option<ClientQueue>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			thread: None,
			client_queue: None,
		})
	}

	pub fn run(&mut self) -> ! {
		unsafe { self.address_space.activate() };
		let s = self as *const _;
		self.thread.as_mut().unwrap().resume(s)
	}

	pub fn init_client_queue(
		&mut self,
		address: *const Page,
		submit_p2size: u8,
		completion_p2size: u8,
	) -> Result<(), NewQueueError> {
		match self.client_queue.as_ref() {
			Some(_) => Err(NewQueueError::QueueAlreadyExists(core::ptr::null())), // TODO return start of queue
			None => {
				let queue = ClientQueue::new(submit_p2size.into(), completion_p2size.into())
					.map_err(NewQueueError::NewClientQueueError)?;
				unsafe {
					self.address_space
						.map(address, queue.frames(), RWX::RW, self.hint_color)
						.map_err(NewQueueError::MapError)?;
				}
				self.client_queue = Some(queue);
				Ok(())
			}
		}
	}

	pub fn poll_client_queue(&mut self) -> Result<(), PollQueueError> {
		let queue = self.client_queue.as_mut().ok_or(PollQueueError::NoQueue)?;
		const OP_SYSLOG: u8 = 127;
		while let Some(e) = queue.pop_submission() {
			match e.opcode {
				OP_SYSLOG => {
					let ptr = usize::from_le_bytes(e.data[7..15].try_into().unwrap());
					let len = usize::from_le_bytes(e.data[15..23].try_into().unwrap());
					let s = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
					info!("{}", core::str::from_utf8(s).unwrap());
				}
				_ => todo!(
					"handle erroneous opcodes (opcode {}, userdata {})",
					e.opcode,
					e.user_data
				),
			}
		}
		Ok(())
	}

	// FIXME wildly unsafe!
	pub fn current<'a>() -> &'a mut Self {
		arch::current_process()
	}
}

impl Drop for Process {
	fn drop(&mut self) {
		todo!()
	}
}

pub struct ProcessID {
	index: u32,
}

#[derive(Debug)]
pub enum NewQueueError {
	QueueAlreadyExists(*const Page),
	NewClientQueueError(NewClientQueueError),
	MapError(MapError),
}

#[derive(Debug)]
pub enum PollQueueError {
	NoQueue,
}
