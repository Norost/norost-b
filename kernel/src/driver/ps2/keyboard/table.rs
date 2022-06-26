use crate::object_table::{Error, Object, Ticket};
use alloc::{boxed::Box, sync::Arc};

pub(super) struct KeyboardTable;

impl Object for KeyboardTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"scancodes" => Ok(Arc::new(ScancodeReader)),
			_ => Err(Error::DoesNotExist),
		})
	}
}

struct ScancodeReader;

impl Object for ScancodeReader {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		if length == 0 {
			Ticket::new_complete(Ok([].into()))
		} else if let Some(s) = super::EVENTS.lock().pop() {
			Ticket::new_complete(Ok(<[u8; 4]>::from(s).into()))
		} else {
			let (ticket, waker) = Ticket::new();
			super::SCANCODE_READERS.lock().push(waker);
			ticket
		}
	}
}
