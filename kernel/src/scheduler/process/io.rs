//! # I/O with user processes

use crate::memory::frame;
use crate::memory::r#virtual::RWX;
use crate::memory::Page;
use core::future::Future;
use core::ptr::NonNull;
use norostb_kernel::io::*;

pub use norostb_kernel::io::Queue;

pub enum CreateQueueError {
	TooLarge,
	OutOfMemory(frame::AllocateContiguousError),
}

pub enum ProcessQueueError {
	InvalidAddress,
}

pub enum WaitQueueError {
	InvalidAddress,
}

const MAX_SIZE_P2: u8 = 15;

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
		let base = base.unwrap(); // TODO
		unsafe {
			self.address_space
				.map(
					base.as_ptr(),
					frame::PageFrameIter { base: frame, count },
					RWX::RW,
					self.hint_color,
				)
				.unwrap();
		}
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
		unsafe {
			let (req_mask, resp_mask) = (queue.requests_mask, queue.responses_mask);
			while let Ok(e) = queue.request_ring_mut().dequeue(req_mask) {
				let mut push_resp = |value| {
					let resps = queue.response_ring_mut();
					// It is the responsibility of the user process to ensure no more requests are in
					// flight than there is space for responses.
					let _ = resps.enqueue(
						resp_mask,
						Response {
							user_data: e.user_data,
							value,
						},
					);
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
						let path =
							unsafe { core::slice::from_raw_parts(path_ptr, path_len).into() };
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
						let info =
							unsafe { &mut *(info as *mut norostb_kernel::syscall::ObjectInfo) };
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
						use crate::object_table::JobType;
						use core::time::Duration;
						let handle = e.arguments_32[0];
						let job = e.arguments_ptr[0] as *mut super::super::syscall::FfiJob;

						let tbl = self.objects[handle as usize].clone().as_table().unwrap();
						let job = unsafe { &mut *job };

						let copy_to = unsafe {
							core::slice::from_raw_parts_mut(
								job.buffer.unwrap().as_ptr(),
								job.buffer_size.try_into().unwrap(),
							)
						};
						let timeout = Duration::new(0, 0);
						let Ok(Ok(info)) = super::super::block_on_timeout(tbl.take_job(timeout), timeout) else {
							push_resp(-1);
							continue;
						};
						job.ty = info.ty.into();
						job.flags = info.flags;
						job.job_id = info.job_id;
						job.handle = info.handle;
						job.operation_size = info.operation_size;
						job.from_anchor = info.from_anchor;
						job.from_offset = info.from_offset;
						job.query_id = info.query_id;
						match info.ty {
							JobType::Open | JobType::Create | JobType::Write | JobType::Query => {
								let size = usize::try_from(info.operation_size).unwrap();
								assert!(copy_to.len() >= size, "todo");
								copy_to[..size].copy_from_slice(&info.buffer[..size]);
							}
							JobType::Open
							| JobType::Read
							| JobType::Write
							| JobType::QueryNext
							| JobType::Seek => {}
						}

						push_resp(0);
					}
					Request::FINISH_JOB => {
						let handle = e.arguments_32[0];
						let job = e.arguments_ptr[0] as *mut super::super::syscall::FfiJob;

						let tbl = self.objects[handle as usize].clone().as_table().unwrap();
						let job = unsafe { job.read() };

						tbl.finish_job(job.try_into().unwrap()).unwrap();

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
					op => {
						warn!("Unknown I/O queue operation {}", op);
						push_resp(-1);
					}
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
