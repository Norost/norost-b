use crate::Handle;
use core::{cell::RefCell, ops::Deref};
use nora_stream_table::{Buffers, ServerQueue, Slice};
use norostb_rt::{
	self as rt,
	io::{Pow2Size, SeekFrom},
};

pub use nora_stream_table::JobId;

pub struct StreamTable {
	queue: RefCell<ServerQueue>,
	buffers: Buffers,
	notify: rt::Object,
	table: rt::Object,
	// Keep a handle around as Root objects use weak references
	public: rt::Object,
}

impl StreamTable {
	/// Create a `StreamTable` with the given memory object as backing store.
	pub fn new(buffers: &rt::Object, block_size: Pow2Size, max_request_mem: u32) -> Self {
		let tbl = rt::Object::new(rt::NewObject::StreamTable {
			allow_sharing: true,
			buffer_mem: buffers.as_raw(),
			buffer_mem_block_size: block_size,
			max_request_mem,
		})
		.unwrap();

		let (queue, size) = tbl.map_object(None, rt::io::RWX::RW, 0, 4096).unwrap();
		assert_eq!(size, 4096, "queue has unexpected size");
		let queue = unsafe { ServerQueue::new(queue) };

		let (buffers, buffers_size) = buffers
			.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
			.unwrap();
		let block_size = u32::try_from(block_size).unwrap();
		let buffers = unsafe { Buffers::new(buffers, buffers_size, block_size) };
		for i in 0..(buffers_size / block_size as usize)
			.try_into()
			.unwrap_or(u32::MAX)
		{
			buffers.dealloc(queue.buffer_head_ref(), i);
		}

		let notify = tbl.open(b"notify").unwrap();
		let public = tbl.open(b"public").unwrap();
		Self {
			queue: queue.into(),
			buffers,
			notify,
			table: tbl,
			public,
		}
	}

	pub fn public(&self) -> &rt::Object {
		&self.public
	}

	pub fn dequeue<'a>(&'a self) -> Option<(Handle, Request)> {
		type R = nora_stream_table::Request;
		let (h, r) = self.queue.borrow_mut().dequeue()?;
		let r = match r {
			R::Read {
				job_id,
				amount,
				peek,
			} => Request::Read {
				job_id,
				amount,
				peek,
			},
			R::Write { job_id, data } => Request::Write {
				job_id,
				data: self.get_owned_buf(data),
			},
			R::GetMeta { job_id, property } => Request::GetMeta {
				job_id,
				property: Property(self.get_owned_buf(property)),
			},
			R::SetMeta {
				job_id,
				property_value,
			} => Request::SetMeta {
				job_id,
				property_value: PropertyValue(self.get_owned_buf(property_value)),
			},
			R::Open { job_id, path } => Request::Open {
				job_id,
				path: self.get_owned_buf(path),
			},
			R::Create { job_id, path } => Request::Create {
				job_id,
				path: self.get_owned_buf(path),
			},
			R::Destroy { job_id, path } => Request::Destroy {
				job_id,
				path: self.get_owned_buf(path),
			},
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
		Some((h, r))
	}

	pub fn enqueue(&self, job_id: JobId, response: Response) {
		type R = nora_stream_table::Response;
		let r = match response {
			Response::Error(e) => R::Error(e as _),
			Response::Amount(n) => R::Amount(n),
			Response::Position(n) => R::Position(n),
			Response::Data(d) => R::Slice(Slice {
				offset: d.offset().try_into().unwrap(),
				length: d.len().try_into().unwrap(),
			}),
			Response::Handle(h) => R::Handle(h),
		};
		self.queue.borrow_mut().try_enqueue(job_id, r).unwrap();
	}

	#[inline(always)]
	pub fn notifier(&self) -> &rt::Object {
		&self.notify
	}

	pub fn wait(&self) {
		self.notify.read(&mut []).unwrap();
	}

	pub fn flush(&self) {
		self.notify.write(&[]).unwrap();
	}

	pub fn alloc(&self, size: usize) -> Option<Data<'_>> {
		self.buffers
			.alloc(self.queue.borrow_mut().buffer_head_ref(), size)
			.map(|data| Data { table: self, data })
	}

	fn get_owned_buf(&self, slice: nora_stream_table::Slice) -> Data<'_> {
		Data {
			table: self,
			data: self.buffers.get(slice),
		}
	}
}

pub enum Request<'a> {
	Read {
		job_id: JobId,
		amount: u32,
		peek: bool,
	},
	Write {
		job_id: JobId,
		data: Data<'a>,
	},
	GetMeta {
		job_id: JobId,
		property: Property<'a>,
	},
	SetMeta {
		job_id: JobId,
		property_value: PropertyValue<'a>,
	},
	Open {
		job_id: JobId,
		path: Data<'a>,
	},
	Close,
	Create {
		job_id: JobId,
		path: Data<'a>,
	},
	Destroy {
		job_id: JobId,
		path: Data<'a>,
	},
	Seek {
		job_id: JobId,
		from: SeekFrom,
	},
	Share {
		job_id: JobId,
		share: rt::Object,
	},
}

pub enum Response<'a> {
	Error(rt::Error),
	Amount(u32),
	Position(u64),
	Data(Data<'a>),
	Handle(Handle),
}

pub struct Data<'a> {
	table: &'a StreamTable,
	data: nora_stream_table::Data<'a>,
}

impl<'a> Data<'a> {
	pub fn manual_drop(self) {
		self.data
			.manual_drop(self.table.queue.borrow().buffer_head_ref());
	}
}

impl<'a> Deref for Data<'a> {
	type Target = nora_stream_table::Data<'a>;

	#[inline(always)]
	fn deref(&self) -> &Self::Target {
		&self.data
	}
}

/* Something seems broken with match, drop and lifetimes, try uncommenting this and
 * build virtio_gpu to see the issue
impl<'a> Drop for Data<'a> {
	fn drop(&mut self) {
		core::mem::replace(&mut self.data, self.table.buffers.alloc_empty())
			.manual_drop(self.table.queue.borrow().buffer_head_ref());
	}
}
*/

pub struct Property<'a>(Data<'a>);

impl<'a> Property<'a> {
	#[inline]
	pub fn get<'b>(&self, buf: &'b mut [u8]) -> &'b mut [u8] {
		let l = buf.len();
		let buf = &mut buf[..self.0.len().min(l)];
		self.0.copy_to_untrusted(0, buf);
		buf
	}

	pub fn manual_drop(self) {
		self.0.manual_drop()
	}

	#[inline(always)]
	pub fn into_inner(self) -> Data<'a> {
		self.0
	}
}

pub struct PropertyValue<'a>(Data<'a>);

impl<'a> PropertyValue<'a> {
	#[inline]
	pub fn try_get<'b>(
		&self,
		buf: &'b mut [u8],
	) -> Result<(&'b mut [u8], &'b mut [u8]), InvalidPropertyValue> {
		let l = buf.len();
		let buf = &mut buf[..self.0.len().min(l)];
		self.0.copy_to_untrusted(0, buf);
		buf.split_first_mut()
			.and_then(|(&mut l, b)| (usize::from(l) <= b.len()).then(|| b.split_at_mut(l.into())))
			.ok_or(InvalidPropertyValue)
	}

	pub fn manual_drop(self) {
		self.0.manual_drop()
	}

	#[inline(always)]
	pub fn into_inner(self) -> Data<'a> {
		self.0
	}
}

#[derive(Debug)]
pub struct InvalidPropertyValue;
