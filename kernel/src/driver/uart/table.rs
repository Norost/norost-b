use {
	super::Uart,
	crate::{
		object_table::{Error, Object, Ticket, TicketWaker},
		sync::SpinLock,
	},
	alloc::{boxed::Box, sync::Arc, vec::Vec},
};

/// Table with all UART devices.
pub struct UartTable;

static PENDING_READS: [SpinLock<Vec<TicketWaker<Box<[u8]>>>>; 1] = [SpinLock::new(Vec::new())];

impl Object for UartTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		if path == b"0" {
			Ticket::new_complete(Ok(Arc::new(UartId::new(0))))
		} else {
			Ticket::new_complete(Err(Error::DoesNotExist))
		}
	}
}

pub struct UartId {
	id: u8,
}

impl UartId {
	fn new(id: u8) -> Self {
		Self { id }
	}
}

impl Object for UartId {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		if length == 0 {
			return Ticket::new_complete(Ok([].into()));
		}
		let mut uart = super::get(self.id.into());
		if let Some(r) = uart.try_read() {
			Ticket::new_complete(Ok([r].into()))
		} else {
			let (ticket, waker) = Ticket::new();
			PENDING_READS[usize::from(self.id)].isr_lock().push(waker);
			uart.enable_interrupts(Uart::INTERRUPT_DATA_AVAILABLE);
			ticket
		}
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		// TODO make write non-blocking.
		let mut uart = super::get(self.id.into());
		data.iter().for_each(|&c| uart.send(c));
		Ticket::new_complete(Ok(data.len().try_into().unwrap()))
	}
}

pub(super) fn irq_handler() {
	for (uart_id, queue) in PENDING_READS.iter().enumerate() {
		let mut uart = super::get(uart_id.into());
		let mut rd = queue.isr_lock();
		while let Some(r) = rd.pop() {
			if let Some(b) = uart.try_read() {
				r.isr_complete(Ok([b].into()));
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
