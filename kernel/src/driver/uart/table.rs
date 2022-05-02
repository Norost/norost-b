use super::Uart;
use crate::object_table::{Error, NoneQuery, Object, Query, Ticket, TicketWaker};
use crate::sync::IsrSpinLock;
use alloc::{boxed::Box, sync::Arc, vec::Vec};

/// Table with all UART devices.
pub struct UartTable;

static PENDING_READS: [IsrSpinLock<Vec<TicketWaker<Box<[u8]>>>>; 1] =
	[IsrSpinLock::new(Vec::new())];

impl Object for UartTable {
	fn query(self: Arc<Self>, prefix: Vec<u8>, tags: &[u8]) -> Ticket<Box<dyn Query>> {
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
}

#[derive(Clone, Copy)]
pub struct UartId(u8);

impl UartId {
	pub fn new(id: u8) -> Self {
		Self(id)
	}
}

impl Object for UartId {
	fn read(&self, _offset: u64, length: usize) -> Ticket<Box<[u8]>> {
		if length == 0 {
			Ticket::new_complete(Ok([].into()))
		} else {
			let mut uart = super::get(self.0.into());
			if let Some(r) = uart.try_read() {
				Ticket::new_complete(Ok([r].into()))
			} else {
				let (ticket, waker) = Ticket::new();
				PENDING_READS[usize::from(self.0)].lock().push(waker);
				uart.enable_interrupts(Uart::INTERRUPT_DATA_AVAILABLE);
				ticket
			}
		}
	}

	fn write(&self, _offset: u64, data: &[u8]) -> Ticket<usize> {
		// TODO make write non-blocking.
		let mut uart = super::get(self.0.into());
		data.iter().for_each(|&c| uart.send(c));
		Ticket::new_complete(Ok(data.len()))
	}
}

pub(super) fn irq_handler() {
	for (uart_id, queue) in PENDING_READS.iter().enumerate() {
		let mut uart = super::get(uart_id.into());
		let mut rd = queue.isr_lock();
		while let Some(r) = rd.pop() {
			if let Some(b) = uart.try_read() {
				r.complete(Ok([b].into()));
			} else {
				rd.push(r);
				break;
			}
		}
		if rd.is_empty() {
			uart.disable_interrupts(Uart::INTERRUPT_DATA_AVAILABLE);
		}
	}
}
