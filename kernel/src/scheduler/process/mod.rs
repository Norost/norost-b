mod elf;
mod io;
mod table;

use super::{MemoryObject, Thread};
use crate::{
	arch,
	memory::{
		frame::{self, AllocateHints},
		r#virtual::{AddressSpace, MapError, UnmapError, RWX},
		Page,
	},
	object_table::{AnyTicket, Object},
	sync::{Mutex, SpinLock},
	util::{erase_handle, unerase_handle},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use arena::Arena;
use core::{num::NonZeroUsize, ptr::NonNull};
use norostb_kernel::Handle;

pub use table::post_init;

pub struct Process {
	address_space: SpinLock<AddressSpace>,
	hint_color: u8,
	threads: SpinLock<Arena<Arc<Thread>, u8>>,
	objects: Mutex<Arena<Arc<dyn Object>, u8>>,
	io_queues: Mutex<Vec<io::Queue>>,
}

struct PendingTicket {
	user_data: u64,
	data_ptr: *mut u8,
	data_len: usize,
	ticket: AnyTicket,
}

impl Process {
	fn new() -> Result<Self, frame::AllocateError> {
		Ok(Self {
			address_space: SpinLock::new(AddressSpace::new()?),
			hint_color: 0,
			threads: Default::default(),
			objects: Default::default(),
			io_queues: Default::default(),
		})
	}

	pub unsafe fn activate_address_space(&self) {
		unsafe { self.address_space.isr_lock().activate() };
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
			.map_object(base, object.into(), rwx, 0, usize::MAX, self.hint_color)
			.map(|(b, _)| b)
	}

	/// Map a memory object to a memory range.
	pub fn map_memory_object_2(
		&self,
		handle: Handle,
		base: Option<NonNull<Page>>,
		rwx: RWX,
		offset: usize,
		max_length: usize,
	) -> Result<(NonNull<Page>, usize), MapError> {
		let obj = self.objects.lock()[unerase_handle(handle)]
			.clone()
			.memory_object()
			.unwrap();
		self.address_space
			.lock()
			.map_object(base, obj, rwx, offset, max_length, self.hint_color)
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

	/// Lock & operate on the objects handles held by this process.
	pub fn objects_operate<'a, R, F>(&'a self, f: F) -> R
	where
		F: FnOnce(&mut Arena<Arc<dyn Object>, u8>) -> R,
	{
		f(&mut self.objects.lock())
	}

	/// Operate on a reference to an object.
	pub fn object_apply<R, F>(&self, handle: Handle, f: F) -> Option<R>
	where
		F: FnOnce(&Arc<dyn Object>) -> R,
	{
		self.objects.lock().get(unerase_handle(handle)).map(f)
	}

	/// Create a new object from another object.
	pub fn object_transform_new<R, F>(&self, handle: Handle, f: F) -> Option<Result<Handle, R>>
	where
		F: FnOnce(&Arc<dyn Object>) -> Result<Arc<dyn Object>, R>,
	{
		let mut obj = self.objects.lock();
		let res = f(obj.get(unerase_handle(handle))?);
		Some(res.map(|o| erase_handle(obj.insert(o))))
	}

	/// Spawn a new thread.
	pub fn spawn_thread(self: &Arc<Self>, start: usize, stack: usize) -> Result<Handle, ()> {
		let thread = Arc::new(Thread::new(start, stack, self.clone()).unwrap());
		let weak = Arc::downgrade(&thread);
		let mut threads = self.threads.lock();
		let handle = threads.insert(thread);
		unsafe {
			threads[handle].set_handle(erase_handle(handle));
		}
		drop(threads);
		super::round_robin::insert(weak);
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

	/// Begin preparations to destroy this process.
	///
	/// This will stop all threads and remove all handles to objects.
	pub fn prepare_destroy(&self) {
		// FIXME actually stop threads.
		// This is necessary to ensure no threads create more handles in the meantime
		self.objects.lock().clear();
	}

	/// Destroy this process.
	///
	/// # Safety
	///
	/// The caller may *not* be using any resources of this process, especially the address space
	/// or a thread!
	#[cfg_attr(debug_assertions, track_caller)]
	pub unsafe fn destroy(self: Arc<Self>) {
		// Destroy all threads
		let mut threads = self.threads.isr_lock();
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
