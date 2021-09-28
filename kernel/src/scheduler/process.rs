use crate::memory::Page;
use crate::memory::frame;
use crate::memory::r#virtual::AddressSpace;

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	//in_ports: Vec<Option<InPort>>,:
	//out_ports: Vec<Option<OutPort>>,
	//named_ports: Box<[ReverseNamedPort]>,
	//threads: Vec<NonNull<Thread>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
		})
	}

	/// # Safety
	///
	/// None of the PPNs may be abuseable by the process.
	pub unsafe fn add_frames(&mut self, address: *const Page, frames: impl ExactSizeIterator<Item = frame::PPN>) -> Result<(), ()> {
		self.address_space.map(address, frames, self.hint_color);
		Ok(())
	}

	pub fn run(&self) -> ! {
		unsafe {
			self.address_space.activate();
			asm!("
				mov ax, (4 * 8) | 3 # ring 3 data with bottom 2 bits set for ring 3
				mov ds, ax
				mov es, ax
				mov fs, ax
				mov gs, ax # SS is handled by iret

				push	(0x4 * 8) | 3	# ss (user data segment)
				push	0x0				# rsp
				pushf					# rflags
				push	(0x3 * 8) | 3	# cs (user code segment)
				push	0x1000			# rip
				rex64 iretq
			", options(noreturn));
		}
	}
}

pub struct ProcessID {
	index: u32,
}
