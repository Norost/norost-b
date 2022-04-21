mod elf;
mod io;

use super::{MemoryObject, Thread};
use crate::arch;
use crate::memory::frame::{self, AllocateHints};
use crate::memory::r#virtual::{AddressSpace, MapError, MemoryObjectHandle, UnmapError, RWX};
use crate::memory::Page;
use crate::object_table::{Object, Query};
use crate::sync::Mutex;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::num::NonZeroUsize;
use core::ptr::NonNull;

pub struct Process {
	address_space: Mutex<AddressSpace>,
	hint_color: u8,
	threads: Mutex<Vec<Arc<Thread>>>,
	objects: Mutex<Vec<Arc<dyn Object>>>,
	queries: Mutex<Vec<Box<dyn Query>>>,
	io_queues: Mutex<Vec<io::Queue>>,
}

impl Process {
	pub fn new() -> Result<Self, frame::AllocateContiguousError> {
		Ok(Self {
			address_space: Mutex::new(AddressSpace::new()?),
			hint_color: 0,
			threads: Default::default(),
			objects: Default::default(),
			queries: Default::default(),
			io_queues: Default::default(),
		})
	}

	pub fn activate_address_space(&self) {
		unsafe { self.address_space.lock().activate() };
	}

	/// Add an object to the process' object table.
	pub fn add_object(&self, object: Arc<dyn Object>) -> Result<ObjectHandle, AddObjectError> {
		let mut objects = self.objects.lock();
		objects.push(object);
		Ok(ObjectHandle(objects.len() - 1))
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object(
		&self,
		base: Option<NonNull<Page>>,
		object: Box<dyn MemoryObject>,
		rwx: RWX,
	) -> Result<NonNull<Page>, MapError> {
		self.address_space
			.lock()
			.map_object(base, object.into(), rwx, self.hint_color)
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object_2(
		&self,
		handle: ObjectHandle,
		base: Option<NonNull<Page>>,
		offset: u64,
		rwx: RWX,
	) -> Result<NonNull<Page>, MapError> {
		let obj = self.objects.lock()[handle.0].memory_object(offset).unwrap();
		self.address_space
			.lock()
			.map_object(base, obj, rwx, self.hint_color)
	}

	/// Unmap a memory object in a memory range. This unmapping may be partial.
	pub fn unmap_memory_object(
		&self,
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		self.address_space.lock().unmap_object(base, count)
	}

	/// Duplicate a reference to an object.
	pub fn duplicate_object_handle(&self, handle: ObjectHandle) -> Option<ObjectHandle> {
		let mut objects = self.objects.lock();
		if let Some(obj) = objects.get(handle.0) {
			// Honestly I don't understand why this isn't fine but the "correct" notation is. Oh
			// well.
			//self.objects.push(obj.clone());
			let obj = obj.clone();
			objects.push(obj);
			Some((objects.len() - 1).into())
		} else {
			None
		}
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.address_space.lock().get_physical_address(address)
	}

	/// Spawn a new thread.
	pub fn spawn_thread(
		self: Arc<Self>,
		start: usize,
		stack: usize,
	) -> Result<usize, crate::memory::frame::AllocateContiguousError> {
		let thr = Arc::new(Thread::new(start, stack, self.clone())?);
		let thr_weak = Arc::downgrade(&thr);
		let mut threads = self.threads.lock();
		threads.push(thr);
		super::round_robin::insert(thr_weak);
		Ok(threads.len() - 1)
	}

	/// Create an [`AllocateHints`] structure for the given virtual address.
	pub fn allocate_hints(&self, address: *const u8) -> AllocateHints {
		AllocateHints {
			address,
			color: self.hint_color,
		}
	}

	/// Add a thread to this process.
	///
	/// The thread must have this process as a parent, i.e. `thread.process == self`.
	pub fn add_thread(&self, thread: Arc<Thread>) {
		self.threads.lock().push(thread)
	}

	pub fn current() -> Arc<Self> {
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
