use {
	super::PCI,
	crate::{
		memory::{
			frame::{PageFrameIter, PPN},
			r#virtual::RWX,
		},
		object_table::{Object, PageFlags, Ticket, TicketWaker},
		scheduler::MemoryObject,
		sync::SpinLock,
		Error,
	},
	alloc::{
		boxed::Box,
		sync::{Arc, Weak},
		vec::Vec,
	},
	core::mem,
	pci::BaseAddress,
};

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

	fn config_region(&self) -> PPN {
		let pci = PCI.isr_lock();
		let pci = pci.as_ref().unwrap();
		let addr = pci.get_physical_address(self.bus, self.device, 0);
		PPN::try_from_usize(addr).unwrap()
	}
}

unsafe impl MemoryObject for PciDevice {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		f(&[self.config_region()]);
	}

	fn physical_pages_len(&self) -> usize {
		1
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(Default::default(), RWX::RW)
	}
}

impl Object for PciDevice {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"poll" => {
				let o = IrqPollInner { irq_occurred: false.into(), waiting: Vec::new() };
				let o = Arc::new(IrqPoll(SpinLock::new(o)));
				IRQ_LISTENERS.lock().push(Arc::downgrade(&o));
				Ok(o)
			}
			b"bar0" | b"bar1" | b"bar2" | b"bar3" | b"bar4" | b"bar5" => {
				let index = usize::from(path[3] - b'0');

				let pci = PCI.lock();
				let pci = pci.as_ref().unwrap();
				let header = pci.get(self.bus, self.device, 0).unwrap();
				let bar = &header.base_addresses()[index];
				let (size, orig) = bar.size();
				bar.set(orig);

				if let Some(size) = BaseAddress::is_mmio(orig).then(|| size).flatten() {
					let upper = || header.base_addresses().get(index + 1).map(|e| e.get());
					let addr = BaseAddress::address(orig, upper).unwrap();
					let mut frames = PageFrameIter {
						base: PPN::try_from_usize(addr.try_into().unwrap()).unwrap(),
						count: size.get().try_into().unwrap(),
					};
					// FIXME there needs to be a better way to limit the amount of pages.
					frames.count = frames.count.min(1 << 20);
					let r = Arc::new(BarRegion { frames: frames.collect() });
					Ok(r)
				} else {
					Err(Error::CantCreateObject)
				}
			}
			_ => Err(Error::DoesNotExist),
		})
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

struct IrqPollInner {
	/// `true` if an IRQ occured since the last poll.
	irq_occurred: bool,
	/// Tasks waiting for an IRQ to occur.
	waiting: Vec<TicketWaker<Box<[u8]>>>,
}

/// An object that keeps track of IRQs atomically.
pub struct IrqPoll(SpinLock<IrqPollInner>);

impl Object for IrqPoll {
	fn read(self: Arc<Self>, _: usize) -> Ticket<Box<[u8]>> {
		let mut inner = self.0.lock();
		if mem::take(&mut inner.irq_occurred) {
			Ticket::new_complete(Ok([].into()))
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

impl Object for BarRegion {
	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for BarRegion {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		f(&self.frames);
	}

	fn physical_pages_len(&self) -> usize {
		self.frames.len()
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(Default::default(), RWX::RW)
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
				w.isr_complete(Ok([].into()));
			}
		}
	}
}
