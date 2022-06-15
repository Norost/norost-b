use crate::object_table::{Error, Object, QueryIter, Ticket};
use alloc::{boxed::Box, sync::Arc};

pub struct VgaTable;

impl Object for VgaTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"" | b"/" => Ok(Arc::new(QueryIter::new([(*b"enable").into()].into_iter()))),
			b"enable" => Ok(Arc::new(Enable)),
			_ => Err(Error::InvalidData),
		})
	}
}

struct Enable;

impl Object for Enable {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(Ok(if length < 1 {
			[].into()
		} else {
			[b"01"[usize::from(super::is_enabled())]].into()
		}))
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<usize> {
		if let Some(v) = data.last() {
			super::set_enable(b"\00".contains(v));
		}
		Ticket::new_complete(Ok(data.len()))
	}
}
