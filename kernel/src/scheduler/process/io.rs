//! # I/O with user processes

use super::{super::poll, erase_handle, unerase_handle, MemoryObject, PendingTicket, TicketOrJob};
use crate::memory::frame::{self, PageFrame, PageFrameIter, PPN};
use crate::memory::r#virtual::{MapError, RWX};
use crate::memory::Page;
use crate::object_table::{
	self, AnyTicketValue, JobRequest, JobResult, Object, Query, QueryResult,
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ptr::{self, NonNull};
use core::task::Poll;
use core::time::Duration;
pub use norostb_kernel::io::Queue;
use norostb_kernel::io::{Job, ObjectInfo, Request, Response, SeekFrom};

pub enum CreateQueueError {
	TooLarge,
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
		&self,
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

		assert_eq!(count, 1, "TODO");
		let mut frame = None;
		frame::allocate(count, |f| frame = Some(f), 0 as *const _, self.hint_color).unwrap();
		/*
		let frame = frame::allocate_contiguous(count.try_into().unwrap())
			.map_err(CreateQueueError::OutOfMemory)?;
		*/
		let frame = frame.unwrap().base;

		unsafe {
			frame.as_ptr().cast::<Page>().write_bytes(0, count);
		}

		let queue = IoQueue { base: frame, count };

		let base = self
			.address_space
			.lock()
			.map_object(base, Box::new(queue), RWX::RW, self.hint_color)
			.map_err(CreateQueueError::MapError)?;
		self.io_queues.lock().push((
			Queue {
				base: base.cast(),
				requests_mask,
				responses_mask,
			},
			Default::default(),
		));
		Ok(base)
	}

	pub fn process_io_queue(&self, base: NonNull<Page>) -> Result<(), ProcessQueueError> {
		let mut io_queues = self.io_queues.lock();
		let (queue, tickets) = io_queues
			.iter_mut()
			.find(|(q, _)| q.base == base.cast())
			.ok_or(ProcessQueueError::InvalidAddress)?;
		let (req_mask, resp_mask) = (queue.requests_mask, queue.responses_mask);

		let mut objects = self.objects.lock();
		let mut queries = self.queries.lock();

		// Poll tickets first as it may shrink the ticket Vec.
		poll_tickets(queue, tickets, &mut objects, &mut queries);

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
			let mut push_pending = |data_ptr, data_len, ticket| {
				tickets.push(PendingTicket {
					user_data: e.user_data,
					data_ptr,
					data_len,
					ticket,
				})
			};
			match e.ty {
				Request::READ => {
					let handle = unerase_handle(e.arguments_32[0]);
					let data_ptr = e.arguments_64[0] as *mut u8;
					let data_len = e.arguments_64[1] as usize;
					let object = objects.get(handle).unwrap();
					let mut ticket = object.read(0, data_len.try_into().unwrap());
					match poll(&mut ticket) {
						Poll::Pending => push_pending(data_ptr, data_len, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(copy_data_to(data_ptr, data_len, b)),
						Poll::Ready(Err(_)) => push_resp(-1),
					}
				}
				Request::PEEK => {
					let handle = unerase_handle(e.arguments_32[0]);
					let data_ptr = e.arguments_64[0] as *mut u8;
					let data_len = e.arguments_64[1] as usize;
					let object = objects.get(handle).unwrap();
					let mut ticket = object.peek(0, data_len.try_into().unwrap());
					match poll(&mut ticket) {
						Poll::Pending => push_pending(data_ptr, data_len, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(copy_data_to(data_ptr, data_len, b)),
						Poll::Ready(Err(_)) => push_resp(-1),
					}
				}
				Request::WRITE => {
					let handle = unerase_handle(e.arguments_32[0]);
					let data_ptr = e.arguments_64[0] as *const u8;
					let data_len = e.arguments_64[1] as usize;
					let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
					let object = objects.get(handle).unwrap();
					let mut ticket = object.write(0, data);
					match poll(&mut ticket) {
						Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(b.try_into().unwrap()),
						Poll::Ready(Err(_)) => push_resp(-1),
					}
				}
				Request::OPEN => {
					let table = object_table::TableId(e.arguments_32[0]);
					let path_ptr = e.arguments_64[0] as *const u8;
					let path_len = e.arguments_64[1] as usize;
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
					match object_table::open(table, path) {
						Ok(mut ticket) => match poll(&mut ticket) {
							Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
							Poll::Ready(Ok(o)) => {
								push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
							}
							Poll::Ready(Err(_)) => push_resp(-1),
						},
						Err(object_table::GetError::InvalidTableId) => push_resp(-1),
					}
				}
				Request::CREATE => {
					let table = object_table::TableId(e.arguments_32[0]);
					let path_ptr = e.arguments_64[0] as *const u8;
					let path_len = e.arguments_64[1] as usize;
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
					match object_table::create(table, path) {
						Ok(mut ticket) => match poll(&mut ticket) {
							Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
							Poll::Ready(Ok(o)) => {
								push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
							}
							Poll::Ready(Err(_)) => push_resp(-1),
						},
						Err(object_table::CreateError::InvalidTableId) => push_resp(-1),
					}
				}
				Request::QUERY => {
					let table = object_table::TableId(e.arguments_32[0]);
					let path_ptr = e.arguments_64[0] as *const u8;
					let path_len = e.arguments_64[1] as usize;
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len).into() };
					match object_table::query(table, path) {
						Ok(mut ticket) => match poll(&mut ticket) {
							Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
							Poll::Ready(Ok(q)) => {
								push_resp(erase_handle(queries.insert(q)).try_into().unwrap())
							}
							Poll::Ready(Err(_)) => push_resp(-1),
						},
						Err(object_table::QueryError::InvalidTableId) => push_resp(-1),
					}
				}
				Request::QUERY_NEXT => {
					let info = e.arguments_64[0] as *mut ObjectInfo;
					let handle = unerase_handle(e.arguments_32[0]);
					let query = &mut queries[handle];
					match query.next() {
						None => push_resp(0),
						Some(mut ticket) => match poll(&mut ticket) {
							Poll::Pending => push_pending(info.cast(), 0, ticket.into()),
							Poll::Ready(Ok(o)) => push_resp(copy_object_info(info, o)),
							Poll::Ready(Err(_)) => push_resp(0),
						},
					}
				}
				Request::TAKE_JOB => {
					let handle = unerase_handle(e.arguments_32[0]);
					let job = e.arguments_64[0] as *mut Job;
					match objects.get(handle).and_then(|o| o.clone().as_table()) {
						Some(tbl) => {
							let mut ticket = tbl.take_job(Duration::MAX);
							match poll(&mut ticket) {
								Poll::Pending => push_pending(job.cast(), 0, ticket.into()),
								Poll::Ready(Ok(info)) => push_resp(take_job(job, info)),
								Poll::Ready(Err(_)) => push_resp(-1),
							}
						}
						None => push_resp(-1),
					}
				}
				Request::FINISH_JOB => {
					let handle = unerase_handle(e.arguments_32[0]);
					let job = e.arguments_64[0] as *mut Job;

					let tbl = objects[handle].clone().as_table().unwrap();
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
						Job::PEEK => JobResult::Peek {
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
					let handle = unerase_handle(e.arguments_32[0]);
					let direction = e.arguments_8[0];
					let offset = e.arguments_64[0];

					let Ok(from) = SeekFrom::try_from_raw(direction, offset) else {
						warn!("Invalid offset ({}, {})", direction, offset);
						push_resp(-1);
						continue;
					};
					match objects.get(handle) {
						Some(object) => {
							let mut ticket = object.seek(from);
							match poll(&mut ticket) {
								Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
								Poll::Ready(Ok(n)) => push_resp(n as i64),
								Poll::Ready(Err(_)) => push_resp(-1),
							}
						}
						None => push_resp(-1),
					}
				}
				Request::POLL => {
					let handle = unerase_handle(e.arguments_32[0]);
					match objects.get(handle) {
						Some(object) => {
							let mut ticket = object.poll();
							match poll(&mut ticket) {
								Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
								Poll::Ready(Ok(n)) => push_resp(n as i64),
								Poll::Ready(Err(_)) => push_resp(-1),
							}
						}
						None => push_resp(-1),
					}
				}
				Request::CLOSE => {
					let handle = unerase_handle(e.arguments_32[0]);
					push_resp(objects.remove(handle).map_or(-1, |_| 0));
				}
				op => {
					warn!("Unknown I/O queue operation {}", op);
					push_resp(-1);
				}
			}
		}
		Ok(())
	}

	pub fn wait_io_queue(&self, base: NonNull<Page>) -> Result<(), WaitQueueError> {
		let mut io_queues = self.io_queues.lock();
		let (queue, tickets) = io_queues
			.iter_mut()
			.find(|(q, _)| q.base == base.cast())
			.ok_or(WaitQueueError::InvalidAddress)?;

		while queue.responses_available() == 0 {
			let polls = poll_tickets(
				queue,
				tickets,
				&mut self.objects.lock(),
				&mut self.queries.lock(),
			);
			if polls == 0 {
				super::super::Thread::current()
					.unwrap()
					.sleep(core::time::Duration::MAX);
			}
		}
		Ok(())
	}
}

fn poll_tickets(
	queue: &mut Queue,
	tickets: &mut Vec<PendingTicket>,
	objects: &mut arena::Arena<Arc<dyn Object>, u8>,
	queries: &mut arena::Arena<Box<dyn Query>, u8>,
) -> usize {
	let mut polls = 0;
	for i in (0..tickets.len()).rev() {
		match &mut tickets[i].ticket {
			TicketOrJob::Ticket(ticket) => match poll(ticket) {
				Poll::Pending => {}
				Poll::Ready(r) => {
					polls += 1;
					let tk = tickets.swap_remove(i);
					let mut push_resp = |value| push_resp(queue, tk.user_data, value);
					match r {
						Ok(AnyTicketValue::Object(o)) => {
							push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
						}
						Ok(AnyTicketValue::Usize(n)) => push_resp(n as i64),
						Ok(AnyTicketValue::U64(n)) => push_resp(n as i64),
						Ok(AnyTicketValue::Data(b)) => {
							let data = unsafe {
								core::slice::from_raw_parts_mut(tk.data_ptr, tk.data_len)
							};
							let len = b.len().min(data.len());
							data[..len].copy_from_slice(&b[..len]);
							push_resp(len.try_into().unwrap())
						}
						Ok(AnyTicketValue::Query(q)) => {
							push_resp(erase_handle(queries.insert(q)).try_into().unwrap())
						}
						Ok(AnyTicketValue::QueryResult(o)) => {
							push_resp(copy_object_info(tk.data_ptr.cast(), o))
						}
						Err(_) => push_resp(-1),
					}
				}
			},
			TicketOrJob::Job(job) => match poll(job) {
				Poll::Pending => {}
				Poll::Ready(r) => {
					polls += 1;
					let tk = tickets.swap_remove(i);
					let mut push_resp = |value| push_resp(queue, tk.user_data, value);
					match r {
						Ok(info) => push_resp(take_job(tk.data_ptr.cast(), info)),
						Err(_) => push_resp(-1),
					}
				}
			},
		}
	}
	polls
}

fn push_resp(queue: &mut Queue, user_data: u64, value: i64) {
	let resp_mask = queue.responses_mask;
	let resps = unsafe { queue.response_ring_mut() };
	// It is the responsibility of the user process to ensure no more requests are in
	// flight than there is space for responses.
	let _ = unsafe { resps.enqueue(resp_mask, Response { user_data, value }) };
}

fn copy_data_to(to_ptr: *mut u8, to_len: usize, from: Box<[u8]>) -> i64 {
	let data = unsafe { core::slice::from_raw_parts_mut(to_ptr, to_len) };
	let len = from.len().min(data.len());
	data[..len].copy_from_slice(&from[..len]);
	len.try_into().unwrap()
}

fn copy_object_info(info: *mut ObjectInfo, obj: QueryResult) -> i64 {
	let info = unsafe { &mut *info };
	let path_buffer = unsafe { core::slice::from_raw_parts_mut(info.path_ptr, info.path_capacity) };
	let len = obj.path.len().min(path_buffer.len());
	info.path_len = len;
	path_buffer[..len].copy_from_slice(&obj.path[..len]);
	1
}

fn take_job(job: *mut Job, info: (u32, JobRequest)) -> i64 {
	let job = unsafe { &mut *job };

	job.job_id = info.0;

	let mut copy_buf = |p: &[u8]| unsafe {
		let ptr = job.buffer.expect("no buffer ptr");
		let buf =
			core::slice::from_raw_parts_mut(ptr.as_ptr(), job.buffer_size.try_into().unwrap());
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
		JobRequest::Read { handle, amount } | JobRequest::Peek { handle, amount } => {
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
		JobRequest::Close { handle } => {
			job.ty = Job::CLOSE;
			job.handle = handle;
		}
	}

	0
}
