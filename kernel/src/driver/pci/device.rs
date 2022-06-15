use super::PCI;
use crate::memory::frame::{PageFrameIter, PPN};
use crate::object_table::Object;
use crate::object_table::{Ticket, TicketWaker};
use crate::scheduler::MemoryObject;
use crate::sync::SpinLock;
use crate::Error;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::mem;
use pci::BaseAddress;

/// A single PCI device.
pub struct PciDevice {
	bus: u8,
	device: u8,
}

static IRQ_LISTENERS: SpinLock<Vec<Weak<IrqPoll>>> = SpinLock::new(Vec::new());

impl PciDevice {
	pub(super) fn new(bus: u8, device: u8) -> Self {
		Self { bus, device }
	}

	pub fn config_region(&self) -> PPN {
		let pci = PCI.auto_lock();
		let pci = pci.as_ref().unwrap();
		let addr = pci.get_physical_address(self.bus, self.device, 0);
		PPN::try_from_usize(addr).unwrap()
	}
}

unsafe impl MemoryObject for PciDevice {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN])) {
		f(&[self.config_region()])
	}

	fn physical_pages_len(&self) -> usize {
		1
	}
}

impl Object for PciDevice {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if path == b"poll" {
			let o = IrqPollInner {
				irq_occurred: false.into(),
				waiting: Vec::new(),
			};
			let o = Arc::new(IrqPoll(SpinLock::new(o)));
			IRQ_LISTENERS.auto_lock().push(Arc::downgrade(&o));
			Ok(o)
		} else {
			Err(Error::DoesNotExist)
		})
	}

	fn memory_object(self: Arc<Self>, offset: u64) -> Option<Arc<dyn MemoryObject>> {
		if offset == 0 {
			return Some(self);
		}

		let index = usize::try_from(offset - 1).ok()?;
		let pci = PCI.auto_lock();
		let pci = pci.as_ref().unwrap();
		let header = pci.get(self.bus, self.device, 0).unwrap();
		let bar = header.base_addresses().get(index)?;
		let (size, orig) = bar.size();
		bar.set(orig);
		let size = size?;
		if !BaseAddress::is_mmio(orig) {
			return None;
		}
		let upper = || header.base_addresses().get(index + 1).map(|e| e.get());
		let addr = BaseAddress::address(orig, upper).unwrap();
		let mut frames = PageFrameIter {
			base: PPN::try_from_usize(addr.try_into().unwrap()).unwrap(),
			count: size.get().try_into().unwrap(),
		};
		dbg!(frames.count);
		// FIXME there needs to be a better way to limit the amount of pages.
		frames.count = frames.count.min(1 << 20);
		let r = Some(Arc::new(BarRegion {
			frames: frames.collect(),
		}) as Arc<dyn MemoryObject>);
		dbg!("ok");
		r
	}
}

struct IrqPollInner {
	/// `true` if an IRQ occured since the last poll.
	irq_occurred: bool,
	/// Tasks waiting for an IRQ to occur.
	waiting: Vec<TicketWaker<usize>>,
}

/// An object that keeps track of IRQs atomically.
pub struct IrqPoll(SpinLock<IrqPollInner>);

impl Object for IrqPoll {
	fn poll(&self) -> Ticket<usize> {
		let mut inner = self.0.auto_lock();
		if mem::take(&mut inner.irq_occurred) {
			Ticket::new_complete(Ok(0))
		} else {
			let (ticket, waker) = Ticket::new();
			inner.waiting.push(waker);
			ticket
		}
	}
}

/// A single MMIO region pointer to by a BAR of a PCI device.
pub struct BarRegion {
	frames: Box<[PPN]>,
}

unsafe impl MemoryObject for BarRegion {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN])) {
		f(&self.frames)
	}

	fn physical_pages_len(&self) -> usize {
		self.frames.len()
	}
}

pub(super) fn irq_handler() {
	let mut l = IRQ_LISTENERS.isr_lock();
	// Remove all dead listen objects
	for i in (0..l.len()).rev() {
		let Some(poll) = Weak::upgrade(&l[i]) else {
			l.swap_remove(i);
			continue;
		};
		let mut poll = poll.0.isr_lock();
		if poll.waiting.is_empty() {
			// Nothing is waiting, so set a flag to return an event immediately to avoid
			// missing any.
			poll.irq_occurred = true;
		} else {
			// There are waiters, so there is no need to set the flag as the event won't be
			// missed.
			for w in poll.waiting.drain(..) {
				w.isr_complete(Ok(1));
			}
		}
	}
}
