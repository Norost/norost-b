mod elf;

use super::{MemoryObject, Thread};
use crate::arch;
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, RWX, MemoryObjectHandle};
use crate::memory::Page;
use crate::object_table::{Object, Query};
use core::ptr::NonNull;
use alloc::{boxed::Box, vec::Vec, sync::Arc};

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	//in_ports: Vec<Option<InPort>>,:
	//out_ports: Vec<Option<OutPort>>,
	//named_ports: Box<[ReverseNamedPort]>,
	thread: Option<Arc<Thread>>,
	//threads: Vec<NonNull<Thread>>,
	objects: Vec<Arc<dyn Object>>,
	queries: Vec<Box<dyn Query>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			thread: None,
			objects: Default::default(),
			queries: Default::default(),
		})
	}

	pub fn activate_address_space(&self) {
		unsafe { self.address_space.activate() };
	}

	pub fn run(&mut self) -> ! {
		self.activate_address_space();
		self.thread.clone().unwrap().resume()
	}

	/// Add an object to the process' object table.
	pub fn add_object(&mut self, object: Arc<dyn Object>) -> Result<ObjectHandle, AddObjectError> {
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
	#[allow(dead_code)]
	pub fn get_memory_object(&self, handle: MemoryObjectHandle) -> Option<&dyn MemoryObject> {
		self.address_space.get_object(handle)
	}

	/// Get a reference to an object.
	pub fn get_object(&mut self, handle: ObjectHandle) -> Option<&Arc<dyn Object>> {
		self.objects.get(handle.0)
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
