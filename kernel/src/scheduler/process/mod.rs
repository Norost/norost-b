mod elf;

use crate::memory::Page;
use crate::memory::frame;
use crate::memory::r#virtual::AddressSpace;

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	//in_ports: Vec<Option<InPort>>,:
	//out_ports: Vec<Option<OutPort>>,
	//named_ports: Box<[ReverseNamedPort]>,
	thread: Option<super::Thread>,
	//threads: Vec<NonNull<Thread>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			thread: None,
		})
	}

	pub fn run(&mut self) -> ! {
		unsafe { self.address_space.activate() };
		self.thread.as_mut().unwrap().resume()
	}
}

pub struct ProcessID {
	index: u32,
}
