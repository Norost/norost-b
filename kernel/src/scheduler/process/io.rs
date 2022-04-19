//! # I/O with user processes

use super::MemoryObject;
use crate::memory::frame::{self, PageFrame, PageFrameIter, PPN};
use crate::memory::r#virtual::{MapError, RWX};
use crate::memory::Page;
use crate::object_table::{JobRequest, JobResult};
use alloc::boxed::Box;
use core::ptr::NonNull;
use core::time::Duration;
pub use norostb_kernel::io::Queue;
use norostb_kernel::io::{Job, ObjectInfo, Request, Response, SeekFrom};

pub enum CreateQueueError {
	TooLarge,
	OutOfMemory(frame::AllocateContiguousError),
	MapError(MapError),
}

pub enum ProcessQueueError {
	InvalidAddress,
}

pub enum WaitQueueError {
	InvalidAddress,
}

const MAX_SIZE_P2: u8 = 15;

struct IoQueue {
	base: PPN,
	count: usize,
}

impl MemoryObject for IoQueue {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		PageFrameIter {
			base: self.base,
			count: self.count,
		}
		.map(|p| PageFrame { base: p, p2size: 0 })
		.collect()
	}
}

impl super::Process {
	pub fn create_io_queue(
		&mut self,
		base: Option<NonNull<Page>>,
		request_p2size: u8,
		response_p2size: u8,
	) -> Result<NonNull<Page>, CreateQueueError> {
		if request_p2size > MAX_SIZE_P2 || response_p2size > MAX_SIZE_P2 {
			return Err(CreateQueueError::TooLarge);
		}
		let requests_mask = (1 << request_p2size) - 1;
		let responses_mask = (1 << response_p2size) - 1;
		let size = Queue::total_size(requests_mask, responses_mask);
		let count = Page::min_pages_for_bytes(size);
		let frame = frame::allocate_contiguous(count.try_into().unwrap())
			.map_err(CreateQueueError::OutOfMemory)?;

		let queue = IoQueue { base: frame, count };

		let base = self
			.address_space
			.map_object(base, Box::new(queue), RWX::RW, self.hint_color)
			.map_err(CreateQueueError::MapError)?;
		self.io_queues.push(Queue {
			base: base.cast(),
			requests_mask,
			responses_mask,
		});
		Ok(base)
	}

	pub fn process_io_queue(&mut self, base: NonNull<Page>) -> Result<(), ProcessQueueError> {
		let queue = self
			.io_queues
			.iter_mut()
			.find(|q| q.base == base.cast())
			.ok_or(ProcessQueueError::InvalidAddress)?;
		let (req_mask, resp_mask) = (queue.requests_mask, queue.responses_mask);
		while let Ok(e) = unsafe { queue.request_ring_mut().dequeue(req_mask) } {
			let mut push_resp = |value| {
				let resps = unsafe { queue.response_ring_mut() };
				// It is the responsibility of the user process to ensure no more requests are in
				// flight than there is space for responses.
				let _ = unsafe {
					resps.enqueue(
						resp_mask,
						Response {
							user_data: e.user_data,
							value,
						},
					)
				};
			};
			match e.ty {
				Request::READ => {
					// TODO make handles use 32 bit integers
					let handle = super::ObjectHandle(e.arguments_32[0].try_into().unwrap());
					let data_ptr = e.arguments_ptr[0] as *mut u8;
					let data_len = e.arguments_ptr[1];
					let data = unsafe { core::slice::from_raw_parts_mut(data_ptr, data_len) };
					let object = self.objects.get(handle.0).unwrap();
					let ticket = object.read(0, data_len.try_into().unwrap());
					let result = super::super::block_on(ticket);
					match result {
						Ok(b) => {
							let len = b.len().min(data.len());
							data[..len].copy_from_slice(&b[..len]);
							push_resp(len.try_into().unwrap())
						}
						Err(_) => push_resp(-1),
					}
				}
				Request::WRITE => {
					// TODO make handles use 32 bit integers
					let handle = super::ObjectHandle(e.arguments_32[0].try_into().unwrap());
					let data_ptr = e.arguments_ptr[0] as *const u8;
					let data_len = e.arguments_ptr[1];
					let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
					let object = self.objects.get(handle.0).unwrap();
					let ticket = object.write(0, data);
					let result = super::super::block_on(ticket);
					match result {
						Ok(n) => push_resp(n.try_into().unwrap()),
						Err(_) => push_resp(-1),
					}
				}
				Request::OPEN => {
					let table = e.arguments_32[0];
					let path_ptr = e.arguments_ptr[0] as *const u8;
					let path_len = e.arguments_ptr[1];
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
					let ticket =
						crate::object_table::open(crate::object_table::TableId(table), path)
							.unwrap();
					let result = super::super::block_on(ticket);
					match result {
						Ok(o) => {
							self.objects.push(o);
							push_resp(self.objects.len() as isize - 1);
						}
						Err(_) => push_resp(-1),
					}
				}
				Request::CREATE => {
					let table = e.arguments_32[0];
					let tags_ptr = e.arguments_ptr[0] as *const u8;
					let tags_len = e.arguments_ptr[1];
					let tags = unsafe { core::slice::from_raw_parts(tags_ptr, tags_len) };
					let ticket =
						crate::object_table::create(crate::object_table::TableId(table), tags)
							.unwrap();
					let result = super::super::block_on(ticket);
					match result {
						Ok(o) => {
							self.objects.push(o);
							push_resp(self.objects.len() as isize - 1);
						}
						Err(_) => push_resp(-1),
					}
				}
				Request::QUERY => {
					let id = e.arguments_32[0];
					let id = crate::object_table::TableId::from(id);
					let path_ptr = e.arguments_ptr[0] as *const u8;
					let path_len = e.arguments_ptr[1];
					// SAFETY: FIXME
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len).into() };
					let ticket = crate::object_table::query(id, path).unwrap();
					let result = super::super::block_on(ticket);
					match result {
						Ok(query) => {
							self.queries.push(query);
							push_resp(self.queries.len() as isize - 1);
						}
						Err(_) => push_resp(-1),
					}
				}
				Request::QUERY_NEXT => {
					// SAFETY: FIXME
					let info = e.arguments_ptr[0];
					let handle = e.arguments_32[0];
					let info = unsafe { &mut *(info as *mut ObjectInfo) };
					let path_buffer = unsafe {
						core::slice::from_raw_parts_mut(info.path_ptr, info.path_capacity)
					};
					let query = &mut self.queries[handle as usize];
					match query.next() {
						None => push_resp(0),
						Some(ticket) => {
							if let Ok(obj) = super::super::block_on(ticket) {
								let len = obj.path.len().min(path_buffer.len());
								info.path_len = len;
								path_buffer[..len].copy_from_slice(&obj.path[..len]);
								push_resp(1)
							} else {
								push_resp(0)
							}
						}
					}
				}
				Request::TAKE_JOB => {
					let handle = e.arguments_32[0];
					let job = e.arguments_ptr[0] as *mut Job;

					let tbl = self.objects[handle as usize].clone().as_table().unwrap();
					let job = unsafe { &mut *job };

					let timeout = Duration::MAX;
					let Ok(Ok(info)) = super::super::block_on_timeout(tbl.take_job(timeout), timeout) else {
						push_resp(-1);
						continue;
					};
					job.job_id = info.0;

					let mut copy_buf = |p: &[u8]| unsafe {
						let ptr = job.buffer.expect("no buffer ptr");
						let buf = core::slice::from_raw_parts_mut(
							ptr.as_ptr(),
							job.buffer_size.try_into().unwrap(),
						);
						buf[..p.len()].copy_from_slice(p);
						job.operation_size = p.len().try_into().unwrap();
					};

					match info.1 {
						JobRequest::Open { path } => {
							job.ty = Job::OPEN;
							copy_buf(&path);
						}
						JobRequest::Create { path } => {
							job.ty = Job::CREATE;
							copy_buf(&path);
						}
						JobRequest::Read { handle, amount } => {
							job.ty = Job::READ;
							job.handle = handle;
							job.operation_size = amount.try_into().unwrap();
						}
						JobRequest::Write { handle, data } => {
							job.ty = Job::WRITE;
							job.handle = handle;
							let len = data.len().min(job.buffer_size.try_into().unwrap());
							copy_buf(&data[..len]);
						}
						JobRequest::Seek { handle, from } => {
							job.ty = Job::SEEK;
							job.handle = handle;
							(job.from_anchor, job.from_offset) = from.into_raw();
						}
						JobRequest::Query { filter } => {
							job.ty = Job::QUERY;
							copy_buf(&filter);
						}
						JobRequest::QueryNext { handle } => {
							job.ty = Job::QUERY_NEXT;
							job.handle = handle;
						}
					}

					push_resp(0);
				}
				Request::FINISH_JOB => {
					let handle = e.arguments_32[0];
					let job = e.arguments_ptr[0] as *mut Job;

					let tbl = self.objects[handle as usize].clone().as_table().unwrap();
					let job = unsafe { job.read() };

					let get_buf = || unsafe {
						let ptr = job.buffer.unwrap_or(NonNull::dangling());
						core::slice::from_raw_parts(
							ptr.as_ptr(),
							job.buffer_size.try_into().unwrap(),
						)
					};

					let job_id = job.job_id;
					let job = match job.ty {
						Job::OPEN => JobResult::Open { handle: job.handle },
						Job::CREATE => JobResult::Create { handle: job.handle },
						Job::READ => JobResult::Read {
							data: get_buf()[..job.operation_size.try_into().unwrap()].into(),
						},
						Job::WRITE => JobResult::Write {
							amount: job.operation_size.try_into().unwrap(),
						},
						Job::SEEK => JobResult::Seek {
							position: job.from_offset,
						},
						Job::QUERY => JobResult::Query { handle: job.handle },
						Job::QUERY_NEXT => JobResult::QueryNext {
							path: get_buf()[..job.operation_size.try_into().unwrap()].into(),
						},
						_ => {
							push_resp(-1);
							continue;
						}
					};

					tbl.finish_job(job, job_id).unwrap();

					push_resp(0);
				}
				Request::SEEK => {
					let handle = e.arguments_32[0];
					let direction = e.arguments_8[0];
					let offset = e.arguments_64[0];
					let write_offset = e.arguments_ptr[0];

					let Ok(from) = SeekFrom::try_from_raw(direction, offset) else {
						warn!("Invalid offset ({}, {})", direction, offset);
						push_resp(-1);
						continue;
					};
					let object = self.objects.get(usize::try_from(handle).unwrap()).unwrap();
					let ticket = object.seek(from);
					let result = super::super::block_on(ticket);
					match result {
						Ok(b) => {
							unsafe {
								(write_offset as *mut u64).write(b);
							}
							push_resp(0)
						}
						Err(_) => push_resp(-1),
					}
				}
				Request::POLL => {
					let handle = e.arguments_32[0];
					let object = self.objects.get(usize::try_from(handle).unwrap()).unwrap();
					let ticket = object.poll();
					let result = super::super::block_on(ticket);
					match result {
						Ok(b) => push_resp(b as isize),
						Err(_) => push_resp(-1),
					}
				}
				op => {
					warn!("Unknown I/O queue operation {}", op);
					push_resp(-1);
				}
			}
		}
		Ok(())
	}

	pub fn wait_io_queue(&mut self, base: NonNull<Page>) -> Result<(), WaitQueueError> {
		let queue = self
			.io_queues
			.iter()
			.find(|q| q.base == base.cast())
			.ok_or(WaitQueueError::InvalidAddress)?;
		while queue.responses_available() == 0 {
			super::super::Thread::current().sleep(core::time::Duration::MAX);
		}
		Ok(())
	}
}
