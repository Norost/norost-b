mod elf;

use super::MemoryObject;
use crate::arch;
#[cfg(feature = "driver-pci")]
use crate::driver::pci::PciDevice;
use crate::ipc::queue::{ClientQueue, NewClientQueueError};
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, Mappable, RWX};
use crate::memory::Page;
use core::ptr::NonNull;
use alloc::{boxed::Box, vec::Vec};

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	//in_ports: Vec<Option<InPort>>,:
	//out_ports: Vec<Option<OutPort>>,
	//named_ports: Box<[ReverseNamedPort]>,
	thread: Option<super::Thread>,
	//threads: Vec<NonNull<Thread>>,
	client_queue: Option<ClientQueue>,
	#[cfg(feature = "driver-pci")]
	pci_devices: Vec<Option<PciDevice>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			thread: None,
			client_queue: None,
			#[cfg(feature = "driver-pci")]
			pci_devices: Vec::new(),
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
						.map(address, queue.frames(), RWX::R, self.hint_color)
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

	/// Map a memory object to a memory range.
	pub fn map_memory_object(
		&mut self,
		base: Option<NonNull<Page>>,
		object: Box<dyn MemoryObject>,
		rwx: RWX,
	) -> Result<(), MapError>
	{
		self.address_space.map_object(base, object.into(), rwx, self.hint_color)
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.address_space.get_physical_address(address)
	}

	#[cfg(feature = "driver-pci")]
	pub fn pci_add_device(&mut self, device: PciDevice, address: *const Page) -> Result<u32, ()> {
		let region = device.config_region();
		unsafe {
			let region = region.base;
			let region = Some(region).into_iter();
			self.address_space
				.map(address, region, RWX::RW, self.hint_color)
				.unwrap();
		}

		match self.pci_devices.iter().position(Option::is_none) {
			Some(i) => {
				self.pci_devices[i] = Some(device);
				Ok(i as u32)
			}
			None => {
				self.pci_devices.push(Some(device));
				Ok((self.pci_devices.len() - 1) as u32)
			}
		}
	}

	#[cfg(feature = "driver-pci")]
	pub fn pci_map_bar(&mut self, device: u32, bar: u8, address: *const Page) -> Result<(), ()> {
		let dev = usize::try_from(device).unwrap();
		let dev = self
			.pci_devices
			.get(dev)
			.and_then(Option::as_ref)
			.ok_or(())?;
		let region = dev.bar_region(bar).map_err(|_| ())?;
		unsafe {
			for i in 0..1 << region.p2size {
				self.address_space
					.map(
						address.wrapping_add(i),
						Some(region.base.skip(i.try_into().unwrap())).into_iter(),
						RWX::RW,
						self.hint_color,
					)
					.unwrap();
			}
		}
		Ok(())
	}

	#[cfg(feature = "driver-pci")]
	pub fn pci_remove_device(&mut self, handle: u32) -> Result<(), ()> {
		if self.pci_devices.len() + 1 == handle as usize {
			self.pci_devices.pop();
			Ok(())
		} else {
			self.pci_devices
				.get_mut(handle as usize)
				.map(|e| *e = None)
				.ok_or(())
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

#[derive(Debug)]
pub enum PollQueueError {
	NoQueue,
}
