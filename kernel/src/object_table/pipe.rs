use super::{Object, Ticket, TicketWaker};
use crate::sync::Mutex;
use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

// 64 KiB should be a reasonable maximum.
const MAX_SIZE: usize = 1 << 16;

/// Create a new unidirectional pipe.
pub fn new() -> [Arc<dyn Object>; 2] {
	let inner = Arc::new(Mutex::default());
	[Arc::new(PipeIn(inner.clone())), Arc::new(PipeOut(inner))]
}

#[derive(Default)]
struct PipeInner {
	buf: VecDeque<u8>,
	// Use VecDequeue so we preserve the read/write order.
	wake_read: VecDeque<TicketWaker<Box<[u8]>>>,
	// FIXME avoid intermediate allocation
	wake_write: VecDeque<(Box<[u8]>, TicketWaker<u64>)>,
	flags: u8,
}

const WRITE_CLOSED: u8 = 1;
const READ_CLOSED: u8 = 2;

struct PipeIn(Arc<Mutex<PipeInner>>);

struct PipeOut(Arc<Mutex<PipeInner>>);

impl Object for PipeIn {
	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		if data.is_empty() {
			return 0.into();
		}
		let max_len = data.len().min(MAX_SIZE);
		let data = &data[..max_len];
		let mut pipe = self.0.lock();
		Ticket::new_complete(if pipe.flags & READ_CLOSED != 0 {
			Ok(0)
		} else if let Some(w) = pipe.wake_read.pop_front() {
			w.complete(Ok(data.into()));
			Ok(max_len as _)
		} else if pipe.buf.len() < MAX_SIZE {
			let len = data.len().min(MAX_SIZE - pipe.buf.len());
			pipe.buf.extend(&data[..len]);
			Ok(len as _)
		} else {
			let (t, w) = Ticket::new();
			pipe.wake_write.push_back((data.into(), w));
			return t;
		})
	}
}

impl Object for PipeOut {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let mut pipe = self.0.lock();
		Ticket::new_complete(if !pipe.buf.is_empty() {
			let len = length.min(pipe.buf.len());
			let (a, b) = pipe.buf.as_slices();
			let mut ret = Vec::with_capacity(len);
			if len > a.len() {
				ret.extend_from_slice(a);
				ret.extend_from_slice(&b[..len - a.len()]);
			} else {
				ret.extend_from_slice(&a[..len]);
			}
			(0..len).for_each(|_| {
				pipe.buf.pop_front();
			});
			Ok(ret.into())
		} else if let Some((b, w)) = pipe.wake_write.pop_front() {
			let mut b = b.to_vec();
			b.resize(length.min(b.len()), 0xfa); // 0xfa so bugs are obvious
			w.complete(Ok(b.len() as _));
			Ok(b.into())
		} else if pipe.flags & WRITE_CLOSED != 0 {
			Ok([].into())
		} else {
			let (t, w) = Ticket::new();
			pipe.wake_read.push_back(w);
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
			.for_each(|w| w.complete(Ok([].into())))
	}
}

impl Drop for PipeOut {
	fn drop(&mut self) {
		let mut pipe = self.0.lock();
		pipe.flags |= READ_CLOSED;
		pipe.wake_write
			.drain(..)
			.for_each(|(_, w)| w.complete(Ok(0)))
	}
}
