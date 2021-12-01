mod elf;

use super::MemoryObject;
use crate::arch;
#[cfg(feature = "driver-pci")]
use crate::driver::pci::PciDevice;
use crate::ipc::queue::{ClientQueue, NewClientQueueError};
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, Mappable, RWX, MemoryObjectHandle};
use crate::memory::Page;
use crate::object_table::{Object, Query};
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
	objects: Vec<Object>,
	queries: Vec<Box<dyn Query>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			thread: None,
			client_queue: None,
			objects: Default::default(),
			queries: Default::default(),
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

	/// Add an object to the process' object table.
	pub fn add_object(&mut self, object: Object) -> Result<ObjectHandle, AddObjectError> {
		self.objects.push(object);
		Ok(ObjectHandle(self.objects.len() - 1))
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object(
		&mut self,
		base: Option<NonNull<Page>>,
		object: Box<dyn MemoryObject>,
		rwx: RWX,
	) -> Result<MemoryObjectHandle, MapError>
	{
		self.address_space.map_object(base, object.into(), rwx, self.hint_color)
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object_2(
		&mut self,
		handle: ObjectHandle,
		base: Option<NonNull<Page>>,
		offset: u64,
		rwx: RWX,
	) -> Result<MemoryObjectHandle, MapError>
	{
		let obj = self.objects[handle.0].memory_object(offset).unwrap();
		self.address_space.map_object(base, obj, rwx, self.hint_color)
	}

	/// Get a reference to a memory object.
	pub fn get_memory_object(&self, handle: MemoryObjectHandle) -> Option<&dyn MemoryObject> {
		self.address_space.get_object(handle)
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.address_space.get_physical_address(address)
	}

	/// Add an object query.
	pub fn add_query(&mut self, query: Box<dyn Query>) -> QueryHandle {
		self.queries.push(query);
		QueryHandle(self.queries.len() - 1)
	}

	/// Get a mutable reference to a query.
	pub fn get_query_mut(&mut self, handle: QueryHandle) -> Option<&mut (dyn Query + 'static)> {
		self.queries.get_mut(handle.0).map(|q| &mut **q)
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

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ObjectHandle(usize);

impl From<ObjectHandle> for usize {
	fn from(h: ObjectHandle) -> Self {
		h.0
	}
}

impl From<usize> for ObjectHandle {
	fn from(n: usize) -> Self {
		Self(n)
	}
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct QueryHandle(usize);

impl From<QueryHandle> for usize {
	fn from(h: QueryHandle) -> Self {
		h.0
	}
}

impl From<usize> for QueryHandle {
	fn from(n: usize) -> Self {
		Self(n)
	}
}

#[derive(Debug)]
pub enum AddObjectError {}
