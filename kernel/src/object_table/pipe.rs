use super::{Error, Object, Ticket, TicketWaker};
use crate::sync::Mutex;
use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

// 64 KiB should be a reasonable maximum.
const MAX_SIZE: usize = 1 << 16;

#[derive(Default)]
pub struct PipeInner {
	buf: VecDeque<u8>,
	// Use VecDequeue so we preserve the read/write order.
	wake_read: VecDeque<TicketWaker<Box<[u8]>>>,
	// FIXME avoid intermediate allocation
	wake_write: VecDeque<(Box<[u8]>, TicketWaker<u64>)>,
}

// Use a single big lock to reduce overhead somewhat.
// Contention shouldn't be very high anyways.
pub struct Pipe(Mutex<PipeInner>);

impl Pipe {
	pub fn new() -> Arc<Self> {
		Arc::new(Self(Default::default()))
	}
}

impl Object for Pipe {
	fn read(self: Arc<Self>, length: usize, peek: bool) -> Ticket<Box<[u8]>> {
		assert!(!peek, "get rid of peek altogether");
		let mut pipe = self.0.lock();
		if !pipe.buf.is_empty() {
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
			Ticket::new_complete(Ok(ret.into()))
		} else if let Some((b, w)) = pipe.wake_write.pop_front() {
			let mut b = b.to_vec();
			b.resize(length.min(b.len()), 0xfa); // 0xfa so bugs are obvious
			w.complete(Ok(b.len() as _));
			Ticket::new_complete(Ok(b.into()))
		} else {
			let (t, w) = Ticket::new();
			pipe.wake_read.push_back(w);
			t
		}
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		let max_len = data.len().min(MAX_SIZE);
		let data = &data[..max_len];
		let mut pipe = self.0.lock();
		if let Some(w) = pipe.wake_read.pop_front() {
			w.complete(Ok(data.into()));
			Ticket::new_complete(Ok(max_len as _))
		} else if pipe.buf.len() < MAX_SIZE {
			let len = data.len().min(MAX_SIZE - pipe.buf.len());
			pipe.buf.extend(&data[..len]);
			Ticket::new_complete(Ok(len as _))
		} else {
			let (t, w) = Ticket::new();
			pipe.wake_write.push_back((data.into(), w));
			t
		}
	}
}
