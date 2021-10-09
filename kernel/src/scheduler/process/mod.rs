mod elf;

use crate::arch;
use crate::ipc::queue::{ClientQueue, NewClientQueueError};
use crate::memory::Page;
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, Mappable, RWX};

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

	pub fn init_client_queue(&mut self, address: *const Page, submit_p2size: u8, completion_p2size: u8) -> Result<(), NewQueueError> {
		match self.client_queue.as_ref() {
			Some(_) => Err(NewQueueError::QueueAlreadyExists(todo!())),
			None => {
				let queue = ClientQueue::new(submit_p2size.into(), completion_p2size.into())
					.map_err(NewQueueError::NewClientQueueError)?;
				unsafe {
					self.address_space.map(address, queue.frames(), RWX::RW, self.hint_color)
						.map_err(NewQueueError::MapError)?;
				}
				dbg!();
				self.client_queue = Some(queue);
				Ok(())
			}
		}
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
