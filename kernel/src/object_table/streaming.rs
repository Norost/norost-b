use super::*;
use crate::{
	memory::{
		frame::{AllocateError, AllocateHints, OwnedPageFrames, PPN},
		r#virtual::{AddressSpace, MapError, RWX},
	},
	object_table::MemoryObject,
	sync::Mutex,
};
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use arena::Arena;
use core::{mem, ptr::NonNull, sync::atomic::Ordering};
use nora_stream_table::{Buffers, ClientQueue, Flags, JobId, Request, Slice};
use norostb_kernel::{io::SeekFrom, syscall::Handle};

pub struct StreamingTable {
	jobs: Mutex<Arena<AnyTicketWaker, ()>>,
	/// Objects that are being shared. They can be taken with an `open` operation
	///
	/// Is `None` if sharing objects is not supported by the server.
	shared: Option<Mutex<Arena<Arc<dyn Object>, ()>>>,
	/// The queue shared with the server.
	queue: Mutex<ClientQueue>,
	/// The shared memory containing the queue.
	queue_mem: Arc<OwnedPageFrames>,
	/// The memory region with buffers that is being shared with the server.
	buffer_mem: Buffers,
	/// The notify singleton for signaling when data is available.
	notify_singleton: Arc<Notify>,
}

/// A wrapper around a [`StreamingTable`], intended for owners to process jobs.
pub struct StreamingTableOwner(Arc<StreamingTable>);

pub enum NewStreamingTableError {
	Alloc(AllocateError),
	Map(MapError),
}

impl StreamingTableOwner {
	pub fn new(
		allow_sharing: bool,
		buffer_mem: Arc<dyn MemoryObject>,
		buffer_mem_block_len: u32,
		hints: AllocateHints,
	) -> Result<Arc<Self>, NewStreamingTableError> {
		let queue_mem = Arc::new(
			OwnedPageFrames::new(1.try_into().unwrap(), hints)
				.map_err(NewStreamingTableError::Alloc)?,
		);
		let (queue, _) = AddressSpace::kernel_map_object(None, queue_mem.clone(), RWX::RW)
			.map_err(NewStreamingTableError::Map)?;
		let queue = unsafe { ClientQueue::new(queue.cast()) };
		queue.buffer_head_ref().store(u32::MAX, Ordering::Relaxed);
		let (buffer_mem, buffer_mem_size) =
			AddressSpace::kernel_map_object(None, buffer_mem, RWX::RW)
				.map_err(NewStreamingTableError::Map)?;
		assert!(buffer_mem_size != 0, "todo");
		Ok(Arc::new(Self(Arc::new_cyclic(|table| StreamingTable {
			jobs: Default::default(),
			shared: allow_sharing.then(Default::default),
			queue: Mutex::new(queue),
			queue_mem,
			buffer_mem: unsafe {
				Buffers::new(buffer_mem.cast(), buffer_mem_size, buffer_mem_block_len)
			},
			notify_singleton: Arc::new(Notify {
				table: table.clone(),
				..Default::default()
			}),
		}))))
	}
}

impl StreamingTable {
	fn submit_job<T, F>(&self, handle: Handle, flags: Flags, f: F) -> Ticket<T>
	where
		F: FnOnce(&mut ClientQueue, JobId) -> Request,
		AnyTicketWaker: From<TicketWaker<T>>,
	{
		let (ticket, ticket_waker) = Ticket::new();
		self.jobs.lock().insert_with(move |h| {
			let mut q = self.queue.lock();
			let r = f(&mut q, JobId::new(h.into_raw().0.try_into().unwrap()));
			q.try_enqueue(handle, flags, r)
				.unwrap_or_else(|e| todo!("{:?}", e));
			ticket_waker.into()
		});
		self.notify_singleton.wake_readers();
		ticket.into()
	}

	fn copy_data_from(&self, queue: &mut ClientQueue, data: &[u8]) -> Slice {
		let (buf, offset) = self
			.buffer_mem
			.alloc(queue.buffer_head_ref(), data.len())
			.unwrap_or_else(|| todo!("no buffers available"));
		let buf = match buf {
			nora_stream_table::Data::Single(b) => b,
		};
		buf.copy_from(0, data);
		Slice {
			offset,
			length: buf.len().try_into().unwrap(),
		}
	}

	fn process_responses(self: &Arc<Self>) {
		let mut q = self.queue.lock();
		let mut j = self.jobs.lock();
		while let Some((job_id, resp)) = q.dequeue() {
			let job_id = arena::Handle::from_raw(job_id.get() as _, ());
			let job = j.remove(job_id).unwrap_or_else(|| todo!("invalid job id"));
			match resp.get() {
				Ok(v) => match job {
					AnyTicketWaker::Object(w) => w.complete(Ok(Arc::new(StreamObject {
						table: Arc::downgrade(self),
						handle: v as _,
					}))),
					AnyTicketWaker::Data(w) => {
						let buf = self
							.buffer_mem
							.get(resp.as_slice().unwrap())
							.next()
							.unwrap_or_else(|| todo!("naughty process"));
						let mut b = Box::new_uninit_slice(buf.len());
						buf.copy_to_uninit(0, &mut b);
						w.complete(Ok(unsafe { Box::<[_]>::assume_init(b) }))
					}
					AnyTicketWaker::U64(w) => w.complete(Ok(v)),
				},
				Err(e) => job.complete_err(Error::from(e)),
			}
		}
	}
}

impl Object for StreamingTableOwner {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"notify" => Ok(self.0.notify_singleton.clone()),
			b"table" => Ok(self.0.clone()),
			&[a, b, c, d] => {
				let h = arena::Handle::from_raw(Handle::from_le_bytes([a, b, c, d]) as _, ());
				self.0
					.shared
					.as_ref()
					.and_then(|s| s.lock().remove(h))
					.ok_or(Error::DoesNotExist)
			}
			_ => Err(Error::InvalidData),
		})
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for StreamingTableOwner {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		self.0.queue_mem.physical_pages(f)
	}

	fn physical_pages_len(&self) -> usize {
		self.0.queue_mem.physical_pages_len()
	}
}

impl Object for StreamingTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(Handle::MAX, Flags::default(), |q, job_id| Request::Open {
			job_id,
			path: self.copy_data_from(q, path),
		})
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.submit_job(Handle::MAX, Flags::default(), |q, job_id| Request::Create {
			job_id,
			path: self.copy_data_from(q, path),
		})
	}
}

impl Drop for StreamingTable {
	fn drop(&mut self) {
		// Wake any waiting tasks so they don't get stuck endlessly.
		let intr = crate::arch::interrupts_enabled();
		for (_, task) in self.jobs.get_mut().drain() {
			if intr {
				task.complete_err(Error::Cancelled)
			} else {
				task.isr_complete_err(Error::Cancelled)
			}
		}
	}
}

struct StreamObject {
	handle: Handle,
	table: Weak<StreamingTable>,
}

impl StreamObject {
	fn with_table<T, F>(&self, f: F) -> Ticket<T>
	where
		F: FnOnce(Arc<StreamingTable>) -> Ticket<T>,
	{
		self.table
			.upgrade()
			.map_or_else(|| Ticket::new_complete(Err(Error::Cancelled)), f)
	}
}

impl Object for StreamObject {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.with_table(|tbl| {
			tbl.submit_job(self.handle, Flags::default(), |q, job_id| Request::Open {
				job_id,
				path: tbl.copy_data_from(q, path),
			})
		})
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.with_table(|tbl| {
			tbl.submit_job(self.handle, Flags::default(), |q, job_id| Request::Create {
				job_id,
				path: tbl.copy_data_from(q, path),
			})
		})
	}

	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		let amount = length.try_into().unwrap_or(u32::MAX);
		self.with_table(|tbl| {
			tbl.submit_job(self.handle, Flags::new(false, true, false), |_, job_id| {
				Request::Read { job_id, amount }
			})
		})
	}

	fn peek(&self, length: usize) -> Ticket<Box<[u8]>> {
		self.with_table(|tbl| {
			let amount = length.try_into().unwrap_or(u32::MAX);
			tbl.submit_job(self.handle, Flags::new(false, false, false), |_, job_id| {
				Request::Read { job_id, amount }
			})
		})
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		self.with_table(|tbl| {
			tbl.submit_job(self.handle, Flags::new(false, true, false), |q, job_id| {
				Request::Write {
					job_id,
					data: tbl.copy_data_from(q, data),
				}
			})
		})
	}

	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		let from = match from {
			SeekFrom::Start(n) => nora_stream_table::SeekFrom::Start(n),
			SeekFrom::Current(n) => nora_stream_table::SeekFrom::Current(n),
			SeekFrom::End(n) => nora_stream_table::SeekFrom::End(n),
		};
		self.with_table(|tbl| {
			tbl.submit_job(self.handle, Flags::default(), |_, job_id| Request::Seek {
				job_id,
				from,
			})
		})
	}

	fn share(&self, share: &Arc<dyn Object>) -> Ticket<u64> {
		self.with_table(|tbl| {
			if let Some(shared) = tbl.shared.as_ref() {
				tbl.submit_job(self.handle, Flags::default(), |_, job_id| Request::Share {
					job_id,
					share: shared
						.lock()
						.insert(share.clone())
						.into_raw()
						.0
						.try_into()
						.unwrap(),
				})
			} else {
				Ticket::new_complete(Err(Error::InvalidOperation))
			}
		})
	}
}

impl Drop for StreamObject {
	fn drop(&mut self) {
		Weak::upgrade(&self.table).map(|table| {
			table
				.queue
				.lock()
				.try_enqueue(self.handle, Flags::default(), Request::Close)
				.unwrap_or_else(|e| todo!("{:?}", e))
		});
	}
}

#[derive(Default)]
struct Notify {
	table: Weak<StreamingTable>,
	wait_read: Mutex<(bool, Vec<TicketWaker<Box<[u8]>>>)>,
}

impl Notify {
	fn wake_readers(&self) {
		let mut q = self.wait_read.lock();
		if let Some(w) = q.1.pop() {
			w.complete(Ok([].into()));
		} else {
			q.0 = true;
		}
	}
}

impl Object for Notify {
	fn read(&self, _length: usize) -> Ticket<Box<[u8]>> {
		let mut q = self.wait_read.lock();
		if mem::take(&mut q.0) {
			Ticket::new_complete(Ok([].into()))
		} else {
			let (t, w) = Ticket::new();
			q.1.push(w);
			t
		}
	}

	fn write(self: Arc<Self>, _data: &[u8]) -> Ticket<u64> {
		Ticket::new_complete(if let Some(tbl) = self.table.upgrade() {
			tbl.process_responses();
			Ok(0)
		} else {
			Err(Error::DoesNotExist)
		})
	}
}
