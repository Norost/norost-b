use crate::Handle;
use core::{cell::RefCell, ops::Deref};
use nora_stream_table::{Buffers, ServerQueue, Slice};
use norostb_rt::{self as rt, io::SeekFrom};

pub use nora_stream_table::{Flags, JobId};

pub struct StreamTable {
	queue: RefCell<ServerQueue>,
	buffers: Buffers,
	notify: rt::Object,
	table: rt::Object,
}

impl StreamTable {
	/// Create a `StreamTable` with the given memory object as backing store.
	pub fn new(buffers: &rt::Object, block_size: u32) -> Self {
		let tbl = rt::Object::new(rt::NewObject::StreamTable {
			allow_sharing: true,
			buffer_mem: buffers.as_raw(),
			buffer_mem_block_size: block_size,
		})
		.unwrap();

		let (queue, size) = tbl.map_object(None, rt::io::RWX::RW, 0, 4096).unwrap();
		assert_eq!(size, 4096, "queue has unexpected size");
		let queue = unsafe { ServerQueue::new(queue) };

		let (buffers, buffers_size) = buffers
			.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
			.unwrap();
		let buffers = unsafe { Buffers::new(buffers, buffers_size, block_size) };
		for i in 0..(buffers_size / block_size as usize)
			.try_into()
			.unwrap_or(u32::MAX)
		{
			buffers.dealloc(queue.buffer_head_ref(), i);
		}

		let notify = tbl.open(b"notify").unwrap();
		Self {
			queue: queue.into(),
			buffers,
			notify,
			table: tbl,
		}
	}

	pub fn public_table(&self) -> rt::Object {
		self.table.open(b"table").unwrap()
	}

	pub fn dequeue<'a>(&'a self) -> Option<(Handle, Flags, Request)> {
		type R = nora_stream_table::Request;
		let (h, f, r) = self.queue.borrow_mut().dequeue()?;
		let r = match r {
			R::Read { job_id, amount } => Request::Read { job_id, amount },
			R::Write { job_id, data } => Request::Write {
				job_id,
				data: self.get_owned_buf(data),
			},
			R::Open { job_id, path } => Request::Open {
				job_id,
				path: self.get_owned_buf(path),
			},
			R::Create { job_id, path } => Request::Create {
				job_id,
				path: self.get_owned_buf(path),
			},
			R::Destroy { job_id } => Request::Destroy { job_id },
			R::Close => Request::Close,
			R::Seek { job_id, from } => Request::Seek {
				job_id,
				from: match from {
					nora_stream_table::SeekFrom::Start(n) => SeekFrom::Start(n),
					nora_stream_table::SeekFrom::Current(n) => SeekFrom::Current(n),
					nora_stream_table::SeekFrom::End(n) => SeekFrom::End(n),
				},
			},
			R::Share { job_id, share } => Request::Share {
				job_id,
				share: self.table.open(&share.to_le_bytes()).unwrap(),
			},
		};
		Some((h, f, r))
	}

	pub fn enqueue(&self, job_id: JobId, response: Response) {
		type R = nora_stream_table::Response;
		let r = match response {
			Response::Error(e) => R::Error(e as _),
			Response::Amount(n) => R::Amount(n),
			Response::Position(n) => R::Position(n),
			Response::Data { data, length } => R::Slice(Slice {
				offset: data.offset,
				length,
			}),
			Response::Handle(h) => R::Handle(h),
		};
		self.queue.borrow_mut().try_enqueue(job_id, r).unwrap();
	}

	pub fn wait(&self) {
		self.notify.read(&mut []).unwrap();
	}

	pub fn flush(&self) {
		self.notify.write(&[]).unwrap();
	}

	pub fn alloc(&self, size: usize) -> Option<Buffer<'_>> {
		self.buffers
			.alloc(self.queue.borrow_mut().buffer_head_ref(), size)
			.map(|(data, offset)| match data {
				nora_stream_table::Data::Single(buffer) => Buffer {
					table: self,
					offset,
					buffer,
				},
			})
	}

	fn get_owned_buf(&self, slice: nora_stream_table::Slice) -> Buffer<'_> {
		Buffer {
			table: self,
			offset: slice.offset,
			buffer: self
				.buffers
				.get(slice)
				.next()
				.unwrap_or(nora_stream_table::Buffer::EMPTY),
		}
	}
}

pub enum Request<'a> {
	Read { job_id: JobId, amount: u32 },
	Write { job_id: JobId, data: Buffer<'a> },
	Open { job_id: JobId, path: Buffer<'a> },
	Close,
	Create { job_id: JobId, path: Buffer<'a> },
	Destroy { job_id: JobId },
	Seek { job_id: JobId, from: SeekFrom },
	Share { job_id: JobId, share: rt::Object },
}

pub enum Response<'a> {
	Error(rt::Error),
	Amount(u32),
	Position(u64),
	Data { data: Buffer<'a>, length: u32 },
	Handle(Handle),
}

pub struct Buffer<'a> {
	table: &'a StreamTable,
	pub offset: u32,
	buffer: nora_stream_table::Buffer<'a>,
}

impl<'a> Buffer<'a> {
	pub fn manual_drop(self) {
		self.table
			.buffers
			.dealloc(self.table.queue.borrow().buffer_head_ref(), self.offset);
	}
}

impl<'a> Deref for Buffer<'a> {
	type Target = nora_stream_table::Buffer<'a>;

	#[inline(always)]
	fn deref(&self) -> &Self::Target {
		&self.buffer
	}
}

/* Something seems broken with match, drop and lifetimes, try uncommenting this and
 * build virtio_gpu to see the issue
impl<'a> Drop for Buffer<'a> {
	fn drop(&mut self) {
		self.table.buffers.dealloc(self.table.queue.buffer_head_ref(), self.offset)
	}
}
*/
