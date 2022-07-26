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
	object_table::{AnyTicket, Error, Object, Ticket, TicketWaker, TinySlice},
	sync::{Mutex, SpinLock},
	util::{erase_handle, unerase_handle},
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use arena::Arena;
use core::{
	num::NonZeroUsize,
	ptr::NonNull,
	sync::atomic::{AtomicU8, Ordering},
};
use norostb_kernel::Handle;

pub use table::post_init;

pub struct Process {
	address_space: SpinLock<AddressSpace>,
	hint_color: u8,
	threads: SpinLock<Arena<Arc<Thread>, u8>>,
	objects: Mutex<Arena<Arc<dyn Object>, u8>>,
	io_queues: Mutex<Vec<io::Queue>>,
	exit_code: AtomicU8,
	wake_on_exit: SpinLock<Vec<TicketWaker<Box<[u8]>>>>,
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
			exit_code: 0.into(),
			wake_on_exit: Default::default(),
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

	/// Add two objects to the process' object table.
	pub fn add_objects(
		&self,
		objects: [Arc<dyn Object>; 2],
	) -> Result<[Handle; 2], AddObjectError> {
		let [a, b] = objects;
		let mut objects = self.objects.lock();
		Ok([
			erase_handle(objects.insert(a)),
			erase_handle(objects.insert(b)),
		])
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
	pub unsafe fn destroy(self: Arc<Self>, exit_code: u8) {
		// Destroy all threads
		let mut threads = self.threads.isr_lock();
		for (_, thr) in threads.drain() {
			// SAFETY: the caller guarantees we're not using any resources of this thread.
			unsafe {
				thr.destroy();
			}
		}
		// ISRs should be disabled as we're outside a thread.
		let mut buf = [0; 2];
		let s = self.encode_status_bin(&threads, &mut buf);
		for w in self.wake_on_exit.isr_lock().drain(..) {
			w.isr_complete(Ok(s.into()));
		}
		self.exit_code.store(exit_code, Ordering::Relaxed);
		// Now we just let the destructors do the rest. Sayonara! :)
	}

	/// Get the current active process.
	pub fn current() -> Option<Arc<Self>> {
		arch::current_process()
	}

	/// Encode the current status of this process.
	fn encode_status_bin<'a>(
		&self,
		threads: &Arena<Arc<Thread>, u8>,
		buf: &'a mut [u8; 2],
	) -> &'a [u8] {
		const IS_DESTROYED: u8 = 1 << 0;
		if threads.is_empty() {
			buf[0] = IS_DESTROYED;
			buf[1] = self.exit_code.load(Ordering::Relaxed);
			buf
		} else {
			buf[0] = 0;
			&buf[..1]
		}
	}
}

impl Drop for Process {
	fn drop(&mut self) {
		// We currently cannot destroy a process in a safe way but we also need to ensure
		// resources are cleaned up properly, so do log it for debugging potential leaks at least.
		debug!("cleaning up process");
	}
}

impl Object for Process {
	fn get_meta(self: Arc<Self>, property: &TinySlice<u8>) -> Ticket<Box<[u8]>> {
		let mut buf = [0; 2];
		Ticket::new_complete(match property.as_ref() {
			// Get the status of the process.
			b"bin/status" => Ok(self
				.encode_status_bin(&self.threads.lock(), &mut buf)
				.into()),
			// Wait for the process to exit and return the status.
			b"bin/wait" => {
				let (t, w) = Ticket::new();
				self.wake_on_exit.lock().push(w);
				return t;
			}
			_ => Err(Error::InvalidData),
		})
	}
}

#[derive(Debug)]
pub enum AddObjectError {}
