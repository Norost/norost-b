use super::{Error, Object, Ticket, TicketWaker};
use crate::sync::Mutex;
use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

// 64 KiB should be a reasonable soft limit.
const SOFT_TOTAL_BYTE_LIMIT: usize = 1 << 16;
const PACKET_MAX_SIZE: usize = 1 << 16;
const WRITE_CLOSED: u8 = 1;
const READ_CLOSED: u8 = 2;

/// Create a new unidirectional message pipe.
///
/// This pipe has a concept of message boundaries.
/// Each message is up to 64 KiB large.
///
/// It is also able to share objects.
///
/// The first object is the input, the second is the output.
pub fn new() -> [Arc<dyn Object>; 2] {
	let inner = Arc::new(Mutex::default());
	[Arc::new(PipeIn(inner.clone())), Arc::new(PipeOut(inner))]
}

#[derive(Default)]
struct PipeInner {
	queue: VecDeque<Box<[u8]>>,
	/// The total amount of bytes enqueued.
	total_bytes: usize,
	// Use VecDequeue so we preserve the read/write order.
	wake_read: VecDeque<(usize, TicketWaker<Box<[u8]>>)>,
	wake_write: VecDeque<TicketWaker<u64>>,
	flags: u8,
}

struct PipeIn(Arc<Mutex<PipeInner>>);

struct PipeOut(Arc<Mutex<PipeInner>>);

impl Object for PipeIn {
	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		if data.len() >= PACKET_MAX_SIZE {
			return Ticket::new_complete(Err(Error::InvalidData));
		}
		let mut pipe = self.0.lock();
		if pipe.flags & READ_CLOSED != 0 {
			return 0.into();
		}
		while let Some((max_len, w)) = pipe.wake_read.pop_front() {
			if max_len >= data.len() {
				w.complete(Ok(data.into()));
				return (data.len() as u64).into();
			} else {
				w.complete(Err(Error::InvalidData)); // TODO better error codes
			}
		}
		pipe.queue.push_back(data.into());
		if pipe.total_bytes < SOFT_TOTAL_BYTE_LIMIT {
			(data.len() as u64).into()
		} else {
			// Block the caller so it doesn't send a massive amount of data.
			let (t, w) = Ticket::new();
			pipe.wake_write.push_back(w);
			t
		}
	}
}

impl Object for PipeOut {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let mut pipe = self.0.lock();
		Ticket::new_complete(if let Some(msg) = pipe.queue.pop_front() {
			if length < msg.len() {
				pipe.queue.push_front(msg);
				// TODO we need more / better error codes
				Err(Error::InvalidData)
			} else {
				pipe.wake_write
					.pop_front()
					.map(|w| w.complete(Ok(msg.len() as _)));
				Ok(msg)
			}
		} else if pipe.flags & WRITE_CLOSED != 0 {
			// FIXME an error like Error::Closed (or similar) would be better.
			Ok([].into())
		} else {
			let (t, w) = Ticket::new();
			pipe.wake_read.push_back((length, w));
			return t;
		})
	}
}

impl Drop for PipeIn {
	fn drop(&mut self) {
		let mut pipe = self.0.lock();
		pipe.flags |= WRITE_CLOSED;
		pipe.wake_read
			.drain(..)
			// ditto
			.for_each(|(_, w)| w.complete(Ok([].into())))
	}
}

impl Drop for PipeOut {
	fn drop(&mut self) {
		let mut pipe = self.0.lock();
		pipe.flags |= READ_CLOSED;
		pipe.wake_write.drain(..).for_each(|w| w.complete(Ok(0)))
	}
}
