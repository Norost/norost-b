mod elf;
mod io;

use super::{MemoryObject, Thread};
use crate::arch;
use crate::memory::frame;
use crate::memory::r#virtual::{AddressSpace, MapError, MemoryObjectHandle, RWX};
use crate::memory::Page;
use crate::object_table::{Object, Query};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ptr::NonNull;

pub struct Process {
	address_space: AddressSpace,
	hint_color: u8,
	threads: Vec<Arc<Thread>>,
	objects: Vec<Arc<dyn Object>>,
	queries: Vec<Box<dyn Query>>,
	io_queues: Vec<io::Queue>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		let address_space = AddressSpace::new()?;
		Ok(Self {
			address_space,
			hint_color: 0,
			threads: Default::default(),
			objects: Default::default(),
			queries: Default::default(),
			io_queues: Default::default(),
		})
	}

	pub fn activate_address_space(&self) {
		unsafe { self.address_space.activate() };
	}

	pub fn run(&mut self) -> ! {
		self.activate_address_space();
		self.threads[0].clone().resume()
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
	) -> Result<MemoryObjectHandle, MapError> {
		self.address_space
			.map_object(base, object.into(), rwx, self.hint_color)
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object_2(
		&mut self,
		handle: ObjectHandle,
		base: Option<NonNull<Page>>,
		offset: u64,
		rwx: RWX,
	) -> Result<MemoryObjectHandle, MapError> {
		let obj = self.objects[handle.0].memory_object(offset).unwrap();
		self.address_space
			.map_object(base, obj, rwx, self.hint_color)
	}

	/// Get a reference to a memory object.
	#[allow(dead_code)]
	pub fn get_memory_object(&self, handle: MemoryObjectHandle) -> Option<&dyn MemoryObject> {
		self.address_space.get_object(handle)
	}

	/// Duplicate a reference to an object.
	pub fn duplicate_object_handle(&mut self, handle: ObjectHandle) -> Option<ObjectHandle> {
		if let Some(obj) = self.objects.get(handle.0) {
			// Honestly I don't understand why this isn't fine but the "correct" notation is. Oh
			// well.
			//self.objects.push(obj.clone());
			let obj = obj.clone();
			self.objects.push(obj);
			Some((self.objects.len() - 1).into())
		} else {
			None
		}
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.address_space.get_physical_address(address)
	}

	/// Spawn a new thread.
	pub fn spawn_thread(
		&mut self,
		start: usize,
		stack: usize,
	) -> Result<usize, crate::memory::frame::AllocateContiguousError> {
		let thr = Arc::new(Thread::new(
			start,
			stack,
			NonNull::new(self as *mut _).unwrap(),
		)?);
		let thr_weak = Arc::downgrade(&thr);
		self.threads.push(thr);
		super::round_robin::insert(thr_weak);
		Ok(self.threads.len() - 1)
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
