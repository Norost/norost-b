//! # I/O with user processes

use super::{super::poll, erase_handle, unerase_handle, MemoryObject, PendingTicket};
use crate::memory::frame::OwnedPageFrames;
use crate::memory::r#virtual::{MapError, UnmapError, RWX};
use crate::memory::Page;
use crate::{
	object_table::{AnyTicketValue, Error, Handle, Object, TinySlice},
	time::Monotonic,
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
	ptr::{self, NonNull},
	task::Poll,
};
use norostb_kernel::io::{self as k_io, Request, Response, SeekFrom};

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

pub(super) struct Queue {
	user_ptr: NonNull<Page>,
	frames: Arc<OwnedPageFrames>,
	requests_mask: u32,
	responses_mask: u32,
	pending: Vec<PendingTicket>,
}

impl Queue {
	fn kernel_io_queue(&self) -> k_io::Queue {
		let mut frame = None;
		self.frames.physical_pages(&mut |f| {
			assert!(frame.is_none() && f.len() == 1, "TODO");
			frame = Some(f[0]);
			true
		});
		k_io::Queue {
			base: NonNull::new(frame.unwrap().as_ptr()).unwrap().cast(),
			requests_mask: self.requests_mask,
			responses_mask: self.responses_mask,
		}
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
		let size = k_io::Queue::total_size(requests_mask, responses_mask);
		let count = Page::min_pages_for_bytes(size);

		// FIXME the user can manually unmap the queue, leading to very bad things.
		// An easy work-around for now is to allow only one page, which is guaranteed to be
		// contiguous and hence we can just use a pointer in identity-mapped space.
		assert_eq!(count, 1, "TODO");
		let frames = Arc::new(
			OwnedPageFrames::new(count.try_into().unwrap(), self.allocate_hints(0 as _)).unwrap(),
		);

		let (user_ptr, _) = self
			.address_space
			.lock()
			.map_object(
				base,
				frames.clone(),
				RWX::RW,
				0,
				usize::MAX,
				self.hint_color,
			)
			.map_err(CreateQueueError::MapError)?;
		self.io_queues.lock().push(Queue {
			user_ptr,
			frames,
			requests_mask,
			responses_mask,
			pending: Default::default(),
		});
		Ok(user_ptr)
	}

	pub fn destroy_io_queue(&self, base: NonNull<Page>) -> Result<(), RemoveQueueError> {
		let queue = {
			let mut queues = self.io_queues.lock();
			let i = queues
				.iter()
				.position(|q| q.user_ptr == base)
				.ok_or(RemoveQueueError::DoesNotExist)?;
			queues.remove(i)
		};

		let size = k_io::Queue::total_size(queue.requests_mask, queue.responses_mask);
		let count = Page::min_pages_for_bytes(size).try_into().unwrap();
		self.address_space
			.lock()
			.unmap_object(base, count)
			.map_err(RemoveQueueError::UnmapError)
	}

	pub fn process_io_queue(&self, base: NonNull<Page>) -> Result<(), ProcessQueueError> {
		let mut io_queues = self.io_queues.lock();
		let queue = io_queues
			.iter_mut()
			.find(|q| q.user_ptr == base)
			.ok_or(ProcessQueueError::InvalidAddress)?;

		let mut objects = self.objects.lock();

		// Poll tickets first as it may shrink the ticket Vec.
		poll_tickets(queue, &mut objects);

		let k_io_queue = queue.kernel_io_queue();
		let tickets = &mut queue.pending;
		let mut queue = k_io_queue;

		while let Ok(e) = unsafe { queue.dequeue_request() } {
			let mut push_resp = |value| {
				// It is the responsibility of the user process to ensure no more requests are in
				// flight than there is space for responses.
				let _ = unsafe {
					queue.enqueue_response(Response {
						user_data: e.user_data,
						value,
					})
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
			let handle = unerase_handle(e.handle);
			let Some(object) = objects.get(handle) else {
				push_resp(Error::InvalidObject as i64);
				continue;
			};
			match e.ty {
				Request::READ => {
					let data_ptr = e.arguments_64[0] as *mut u8;
					let data_len = e.arguments_64[1] as usize;
					let mut ticket = object.clone().read(data_len.try_into().unwrap());
					match poll(&mut ticket) {
						Poll::Pending => push_pending(data_ptr, data_len, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(copy_data_to(data_ptr, data_len, b)),
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::WRITE => {
					let data_ptr = e.arguments_64[0] as *const u8;
					let data_len = e.arguments_64[1] as usize;
					let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
					// TODO crappy workaround for Stream table
					// We should instead pass a reference to the objects list or some Context
					// object.
					let object = object.clone();
					drop(objects);
					let mut ticket = object.write(data);
					objects = self.objects.lock();
					match poll(&mut ticket) {
						Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(b.try_into().unwrap()),
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::OPEN => {
					let path_ptr = e.arguments_64[0] as *const u8;
					let path_len = e.arguments_64[1] as usize;
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
					let mut ticket = object.clone().open(path);
					match poll(&mut ticket) {
						Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
						Poll::Ready(Ok(o)) => {
							push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
						}
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::CREATE => {
					let path_ptr = e.arguments_64[0] as *const u8;
					let path_len = e.arguments_64[1] as usize;
					let path = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
					let mut ticket = object.clone().create(path);
					match poll(&mut ticket) {
						Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
						Poll::Ready(Ok(o)) => {
							push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
						}
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::SEEK => {
					let direction = e.arguments_8[0];
					let offset = e.arguments_64[0];

					let Ok(from) = SeekFrom::try_from_raw(direction, offset) else {
						warn!("Invalid offset ({}, {})", direction, offset);
						push_resp(-1);
						continue;
					};
					let mut ticket = object.seek(from);
					match poll(&mut ticket) {
						Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
						Poll::Ready(Ok(n)) => push_resp(n as i64),
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::CLOSE => {
					// We are not supposed to return a response under any circumstances.
					let _ = objects.remove(handle);
				}
				Request::SHARE => {
					let share = unerase_handle(e.arguments_64[0] as Handle);
					if let Some(shr) = objects.get(share) {
						let mut ticket = object.clone().share(shr);
						match poll(&mut ticket) {
							Poll::Pending => push_pending(ptr::null_mut(), 0, ticket.into()),
							Poll::Ready(Ok(n)) => push_resp(n as i64),
							Poll::Ready(Err(e)) => push_resp(e as i64),
						}
					} else {
						push_resp(Error::InvalidObject as i64)
					}
				}
				Request::GET_META => {
					let [prop_len, val_len, _] = e.arguments_8;
					let [prop_ptr, val_ptr] = e.arguments_64;
					let prop =
						unsafe { TinySlice::from_raw_parts(prop_ptr as *const u8, prop_len) };
					let mut ticket = object.clone().get_meta(prop);
					let (val_ptr, val_len) = (val_ptr as *mut u8, val_len.into());
					match poll(&mut ticket) {
						Poll::Pending => push_pending(val_ptr, val_len, ticket.into()),
						Poll::Ready(Ok(b)) => push_resp(copy_data_to(val_ptr, val_len, b)),
						Poll::Ready(Err(e)) => push_resp(e as i64),
					}
				}
				Request::SET_META => todo!(),
				Request::DESTROY => todo!(),
				op => {
					warn!("Unknown I/O queue operation {}", op);
					push_resp(Error::InvalidOperation as i64);
				}
			}
		}
		Ok(())
	}

	pub fn wait_io_queue(&self, base: NonNull<Page>, timeout: u64) -> Result<(), WaitQueueError> {
		for i in 0..2 {
			let mut io_queues = self.io_queues.lock();
			let queue = io_queues
				.iter_mut()
				.find(|q| q.user_ptr == base)
				.ok_or(WaitQueueError::InvalidAddress)?;

			if queue.kernel_io_queue().responses_available() > 0 {
				break;
			}

			{
				let mut objects = self.objects.lock();
				let polls = poll_tickets(queue, &mut objects);
				if polls > 0 {
					break;
				}
			}

			// Prevent blocking other threads.
			drop(io_queues);

			if i == 0 {
				super::super::Thread::current()
					.unwrap()
					.wait_until(Monotonic::now(), timeout);
			}
		}
		Ok(())
	}
}

fn poll_tickets(queue: &mut Queue, objects: &mut arena::Arena<Arc<dyn Object>, u8>) -> usize {
	let mut polls = 0;
	for i in (0..queue.pending.len()).rev() {
		match poll(&mut queue.pending[i].ticket) {
			Poll::Pending => {}
			Poll::Ready(r) => {
				polls += 1;
				let tk = queue.pending.swap_remove(i);
				let mut push_resp = |value| push_resp(queue, tk.user_data, value);
				match r {
					Ok(AnyTicketValue::Object(o)) => {
						push_resp(erase_handle(objects.insert(o)).try_into().unwrap())
					}
					Ok(AnyTicketValue::U64(n)) => push_resp(n as i64),
					Ok(AnyTicketValue::Data(b)) => {
						let data =
							unsafe { core::slice::from_raw_parts_mut(tk.data_ptr, tk.data_len) };
						let len = b.len().min(data.len());
						data[..len].copy_from_slice(&b[..len]);
						push_resp(len.try_into().unwrap())
					}
					Err(e) => push_resp(e as i64),
				}
			}
		}
	}
	polls
}

fn push_resp(queue: &mut Queue, user_data: u64, value: i64) {
	// It is the responsibility of the user process to ensure no more requests are in
	// flight than there is space for responses.
	let _ = unsafe {
		queue
			.kernel_io_queue()
			.enqueue_response(Response { user_data, value })
	};
}

fn copy_data_to(to_ptr: *mut u8, to_len: usize, from: Box<[u8]>) -> i64 {
	let data = unsafe { core::slice::from_raw_parts_mut(to_ptr, to_len) };
	let len = from.len().min(data.len());
	data[..len].copy_from_slice(&from[..len]);
	len.try_into().unwrap()
}

pub enum RemoveQueueError {
	DoesNotExist,
	UnmapError(UnmapError),
}
