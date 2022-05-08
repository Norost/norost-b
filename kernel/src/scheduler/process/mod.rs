mod elf;
mod io;
mod table;

use super::{MemoryObject, Thread};
use crate::arch;
use crate::memory::frame::{self, AllocateHints};
use crate::memory::r#virtual::{AddressSpace, MapError, UnmapError, RWX};
use crate::memory::Page;
use crate::object_table::{AnyTicket, JobTask, Object, Query};
use crate::sync::Mutex;
use crate::util::{erase_handle, unerase_handle};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use arena::Arena;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use norostb_kernel::Handle;

pub use table::init;

pub struct Process {
	address_space: Mutex<AddressSpace>,
	hint_color: u8,
	threads: Mutex<Arena<Arc<Thread>, u8>>,
	objects: Mutex<Arena<Arc<dyn Object>, u8>>,
	queries: Mutex<Arena<Box<dyn Query>, u8>>,
	io_queues: Mutex<Vec<io::Queue>>,
}

struct PendingTicket {
	user_data: u64,
	data_ptr: *mut u8,
	data_len: usize,
	ticket: TicketOrJob,
}

enum TicketOrJob {
	Ticket(AnyTicket),
	Job(JobTask),
}

impl<T: Into<AnyTicket>> From<T> for TicketOrJob {
	fn from(t: T) -> Self {
		Self::Ticket(t.into())
	}
}

impl From<JobTask> for TicketOrJob {
	fn from(t: JobTask) -> Self {
		Self::Job(t)
	}
}

impl Process {
	fn new() -> Result<Self, frame::AllocateContiguousError> {
		Ok(Self {
			address_space: Mutex::new(AddressSpace::new()?),
			hint_color: 0,
			threads: Default::default(),
			objects: Default::default(),
			queries: Default::default(),
			io_queues: Default::default(),
		})
	}

	pub unsafe fn activate_address_space(&self) {
		unsafe { self.address_space.lock().activate() };
	}

	/// Add an object to the process' object table.
	pub fn add_object(&self, object: Arc<dyn Object>) -> Result<Handle, AddObjectError> {
		let mut objects = self.objects.lock();
		Ok(erase_handle(objects.insert(object)))
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
		handle: Handle,
		base: Option<NonNull<Page>>,
		offset: u64,
		rwx: RWX,
	) -> Result<NonNull<Page>, MapError> {
		let obj = self.objects.lock()[unerase_handle(handle)]
			.clone()
			.memory_object(offset)
			.unwrap();
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
	pub fn duplicate_object_handle(&self, handle: Handle) -> Option<Handle> {
		let mut objects = self.objects.lock();
		if let Some(obj) = objects.get(unerase_handle(handle)) {
			let obj = obj.clone();
			Some(erase_handle(objects.insert(obj)))
		} else {
			None
		}
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.address_space.lock().get_physical_address(address)
	}

	/// Spawn a new thread.
	pub fn spawn_thread(self: &Arc<Self>, start: usize, stack: usize) -> Result<Handle, ()> {
		let mut threads = self.threads.lock();
		let handle = threads.insert_with(|handle| {
			Arc::new(Thread::new(start, stack, self.clone(), erase_handle(handle)).unwrap())
		});
		super::round_robin::insert(Arc::downgrade(&threads.get(handle).unwrap()));
		Ok(erase_handle(handle))
	}

	/// Get a thread.
	pub fn get_thread(&self, handle: Handle) -> Option<Arc<Thread>> {
		self.threads.lock().get(unerase_handle(handle)).cloned()
	}

	/// Remove a thread.
	pub fn remove_thread(&self, handle: Handle) -> Option<Arc<Thread>> {
		let handle = arena::Handle::from_raw(
			(handle & 0xff_ffff).try_into().unwrap(),
			(handle >> 24) as u8,
		);
		self.threads.lock().remove(handle)
	}

	/// Create an [`AllocateHints`] structure for the given virtual address.
	pub fn allocate_hints(&self, address: *const u8) -> AllocateHints {
		AllocateHints {
			address,
			color: self.hint_color,
		}
	}

	/// Destroy this process.
	///
	/// # Safety
	///
	/// The caller may *not* be using any resources of this process, especially the address space
	/// or a thread!
	pub unsafe fn destroy(self: Arc<Self>) {
		// Destroy all threads
		let mut threads = self.threads.lock();
		for (_, thr) in threads.drain() {
			// SAFETY: the caller guarantees we're not using any resources of this thread.
			unsafe {
				thr.destroy();
			}
		}
		// Now we just let the destructors do the rest. Sayonara! :)
	}

	/// Get the current active process.
	pub fn current() -> Option<Arc<Self>> {
		arch::current_process()
	}
}

impl Drop for Process {
	fn drop(&mut self) {
		// We currently cannot destroy a process in a safe way but we also need to ensure
		// resources are cleaned up properly, so do log it for debugging potential leaks at least.
		debug!("cleaning up process");
	}
}

impl Object for Process {}

#[derive(Debug)]
pub enum AddObjectError {}
