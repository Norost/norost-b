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
						let ticket = object.read(0, data_len.try_into().unwrap()).unwrap();
						let result = super::super::block_on(ticket);
						match result {
							Ok(crate::object_table::Data::Bytes(b)) => {
								let len = b.len().min(data.len());
								data[..len].copy_from_slice(&b[..len]);
								push_resp(len.try_into().unwrap())
							}
							Ok(_) => unreachable!("invalid ok result"),
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
						let ticket = object.write(0, data).unwrap();
						let result = super::super::block_on(ticket);
						match result {
							Ok(crate::object_table::Data::Usize(n)) => {
								push_resp(n.try_into().unwrap())
							}
							Ok(_) => unreachable!("invalid ok result"),
							Err(_) => push_resp(-1),
						}
					}
					Request::OPEN => {
						let table = e.arguments_32[0];
						let id = e.arguments_64[0];
						let ticket = crate::object_table::get(
							crate::object_table::TableId(table),
							crate::object_table::Id(id),
						)
						.unwrap();
						let result = super::super::block_on(ticket);
						match result {
							Ok(crate::object_table::Data::Object(o)) => {
								self.objects.push(o);
								push_resp(self.objects.len() as isize - 1);
							}
							Ok(_) => unreachable!("invalid ok result"),
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
							Ok(crate::object_table::Data::Object(o)) => {
								self.objects.push(o);
								push_resp(self.objects.len() as isize - 1);
							}
							Ok(_) => unreachable!("invalid ok result"),
							Err(_) => push_resp(-1),
						}
					}
					Request::QUERY => {
						let id = e.arguments_32[0];
						let id = crate::object_table::TableId::from(id);
						let tags_ptr = e.arguments_ptr[0] as *const u8;
						let tags_len = e.arguments_ptr[1];
						let mut tags_split = [""; 32];
						// SAFETY: FIXME
						let tags = unsafe {
							let mut len = 0;
							for (r, w) in core::slice::from_raw_parts(tags_ptr, tags_len)
								.split(|c| *c == b'&')
								.zip(tags_split.iter_mut())
							{
								*w = core::str::from_utf8(r).unwrap();
								len += 1;
							}
							&tags_split[..len]
						};
						let query = crate::object_table::query(id, tags).unwrap();
						self.queries.push(query);
						push_resp(self.queries.len() as isize - 1);
					}
					Request::QUERY_NEXT => {
						// SAFETY: FIXME
						let info = e.arguments_ptr[0];
						let handle = e.arguments_32[0];
						let info =
							unsafe { &mut *(info as *mut norostb_kernel::syscall::ObjectInfo<'_>) };
						let string_buffer = unsafe {
							core::slice::from_raw_parts_mut(
								info.string_buffer_ptr,
								info.string_buffer_len,
							)
						};
						let query = &mut self.queries[handle as usize];
						match query.next() {
							None => push_resp(0),
							Some(obj) => {
								info.id = obj.id.0;
								info.tags_len = obj.tags.len().try_into().unwrap();
								let mut p = 0;
								for (to, tag) in info.tags_offsets.iter_mut().zip(&*obj.tags) {
									*to = p as u32;
									let q = p + 1 + tag.len();
									if q >= string_buffer.len() {
										// There is not enough space to copy the tag, so just skip it and
										// the remaining tags.
										break;
									}
									string_buffer[p] = tag.len().try_into().unwrap();
									string_buffer[p + 1..q].copy_from_slice(tag.as_bytes());
									p = q;
								}
								push_resp(1)
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
						job.object_id = info.object_id;
						job.operation_size = info.operation_size;
						match info.ty {
							JobType::Create | JobType::Write => {
								let size = usize::try_from(info.operation_size).unwrap();
								assert!(copy_to.len() >= size, "todo");
								copy_to[..size].copy_from_slice(&info.buffer[..size]);
							}
							_ => {}
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
					_ => {
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
