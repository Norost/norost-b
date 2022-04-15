use crate::object_table::{Error, NoneQuery, Object, Query, Table, Ticket};
use alloc::{boxed::Box, sync::Arc};

/// Table with all UART devices.
pub struct UartTable;

impl Table for UartTable {
	fn name(&self) -> &str {
		"uart"
	}

	fn query(self: Arc<Self>, tags: &[u8]) -> Ticket<Box<dyn Query>> {
		match tags {
			&[] => todo!(),
			_ => Ticket::new_complete(Ok(Box::new(NoneQuery))),
		}
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		if path == b"0" {
			Ticket::new_complete(Ok(Arc::new(UartId(0))))
		} else {
			todo!()
		}
	}

	fn create(self: Arc<Self>, _: &[u8]) -> Ticket<Arc<dyn Object>> {
		let e = Error {
			code: 1,
			message: "can't create uart devices".into(),
		};
		Ticket::new_complete(Err(e))
	}
}

impl Object for UartTable {}

pub struct UartId(u8);

impl Object for UartId {
	fn read(&self, _offset: u64, length: usize) -> Ticket<Box<[u8]>> {
		// TODO read more than one byte doofus.
		if let Some(r) = (length > 0)
			.then(|| super::get(self.0.into()).try_read())
			.flatten()
		{
			Ticket::new_complete(Ok([r].into()))
		} else {
			Ticket::new_complete(Ok([].into()))
		}
	}

	fn write(&self, _offset: u64, data: &[u8]) -> Ticket<usize> {
		// TODO make write non-blocking.
		let mut uart = super::get(self.0.into());
		data.iter().for_each(|&c| uart.send(c));
		Ticket::new_complete(Ok(data.len()))
	}
}
