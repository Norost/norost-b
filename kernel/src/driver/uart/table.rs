use super::Uart;
use super::DEVICES;
use crate::object_table::{
	Data, Error, Id, Job, JobTask, NoneQuery, Object, OneQuery, Query, QueryResult, Table, Ticket,
};
use alloc::{boxed::Box, format, string::String, string::ToString, sync::Arc};

/// Table with all UART devices.
pub struct UartTable;

impl Table for UartTable {
	fn name(&self) -> &str {
		"uart"
	}

	fn query(self: Arc<Self>, tags: &[&str]) -> Box<dyn Query> {
		match tags {
			&[] => todo!(),
			_ => Box::new(NoneQuery),
		}
	}

	fn get(self: Arc<Self>, id: Id) -> Ticket {
		if id.0 == 0 {
			Ticket::new_complete(Ok(Data::Object(Arc::new(UartId(id.0.try_into().unwrap())))))
		} else {
			todo!()
		}
	}

	fn create(self: Arc<Self>, _: &[u8]) -> Ticket {
		let e = Error {
			code: 1,
			message: "can't create uart devices".into(),
		};
		Ticket::new_complete(Err(e))
	}

	fn take_job(self: Arc<Self>, _: core::time::Duration) -> JobTask {
		unreachable!("kernel only table")
	}

	fn finish_job(self: Arc<Self>, _: Job) -> Result<(), ()> {
		unreachable!("kernel only table")
	}

	fn cancel_job(self: Arc<Self>, _: Job) {
		unreachable!("kernel only table")
	}
}

impl Object for UartTable {}

pub struct UartId(u8);

impl Object for UartId {
	fn read(&self, _offset: u64, length: u32) -> Result<Ticket, ()> {
		// TODO read more than one byte doofus.
		if let Some(r) = (length > 0)
			.then(|| super::get(self.0.into()).try_read())
			.flatten()
		{
			Ok(Ticket::new_complete(Ok(Data::Bytes([r].into()))))
		} else {
			Ok(Ticket::new_complete(Ok(Data::Bytes([].into()))))
		}
	}

	fn write(&self, _offset: u64, data: &[u8]) -> Result<Ticket, ()> {
		// TODO make write non-blocking.
		let mut uart = super::get(self.0.into());
		data.iter().for_each(|&c| uart.send(c));
		Ok(Ticket::new_complete(Ok(Data::Usize(data.len()))))
	}
}
