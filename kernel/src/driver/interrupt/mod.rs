//! # Allocatable IRQ's for userspace drivers.

use {
	super::apic::{
		io_apic::{self, TriggerMode},
		local_apic,
	},
	crate::{
		arch,
		object_table::{Error, Object, Root, Ticket, TicketWaker},
		sync::SpinLock,
	},
	alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec},
	core::{mem, str},
};

// TODO add a vector and irq type to arch
type InterruptVector = u8;
type InterruptIrq = u8;

// TODO use rwlock of sorts and add interior mutability to Entry.
static LISTENERS: SpinLock<BTreeMap<InterruptVector, Entry>> = SpinLock::new(BTreeMap::new());

struct InterruptTable;

impl Object for InterruptTable {
	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		let (mode, irq) = match path {
			p if p.starts_with(b"edge/") => (TriggerMode::Edge, &p[5..]),
			p if p.starts_with(b"level/") => (TriggerMode::Level, &p[6..]),
			_ => return Error::DoesNotExist.into(),
		};
		let Ok(vector) = arch::allocate_irq() else { return Error::CantCreateObject.into() };
		let irq = match irq {
			b"any" => todo!("alloc any vector"),
			// FIXME avoid vector conflicts
			n if let Some(n) = str::from_utf8(n).ok().and_then(|p| p.parse::<u8>().ok()) => n,
			_ => return Error::DoesNotExist.into(),
		};
		unsafe {
			arch::amd64::set_interrupt_handler(vector.into(), handle_irq);
		}
		LISTENERS.lock().insert(
			vector,
			Entry { mode, irq, triggered: false, wake: Default::default() },
		);
		unsafe {
			io_apic::set_irq(irq, 0, vector, mode, false);
		}
		Ticket::new_complete(Ok(Arc::new(Interrupt(vector))))
	}
}

struct Entry {
	mode: TriggerMode,
	irq: InterruptIrq,
	triggered: bool,
	wake: Vec<TicketWaker<Box<[u8]>>>,
}

struct Interrupt(InterruptVector);

impl Drop for Interrupt {
	fn drop(&mut self) {
		LISTENERS.auto_lock().remove(&self.0).unwrap();
		unsafe { arch::deallocate_irq(self.0) }
	}
}

impl Object for Interrupt {
	fn read(self: Arc<Self>, _: usize) -> Ticket<Box<[u8]>> {
		let mut l = LISTENERS.auto_lock();
		let e = l.get_mut(&self.0).unwrap();
		if mem::take(&mut e.triggered) {
			Ticket::new_complete(Ok([].into()))
		} else {
			let (t, w) = Ticket::new();
			e.wake.push(w);
			t
		}
	}

	fn write(self: Arc<Self>, _: &[u8]) -> Ticket<u64> {
		let mut l = LISTENERS.auto_lock();
		let e = l.get_mut(&self.0).unwrap();
		unsafe {
			if e.mode == TriggerMode::Level {
				io_apic::mask_irq(e.irq, false);
			}
		}
		0.into()
	}
}

pub fn post_init(root: &Root) {
	let tbl = Arc::new(InterruptTable);
	root.add(*b"interrupt", Arc::downgrade(&tbl) as _);
	let _ = Arc::into_raw(tbl);
}

extern "C" fn handle_irq(vector: u32) {
	let mut l = LISTENERS.isr_lock();
	let e = l.get_mut(&(vector as _)).unwrap();
	if let Some(w) = e.wake.pop() {
		w.isr_complete(Ok([].into()));
	} else {
		e.triggered = true;
	}
	if e.mode == TriggerMode::Level {
		unsafe { io_apic::mask_irq(e.irq, true) };
	}
	local_apic::get().eoi.set(0);
}
