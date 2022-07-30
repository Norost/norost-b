//! # Allocatable IRQ's for userspace drivers.

use super::apic::{
	io_apic::{self, TriggerMode},
	local_apic,
};
use crate::{
	arch,
	object_table::{Error, Object, Root, Ticket, TicketWaker},
	sync::SpinLock,
	util,
};
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use arena::Arena;
use core::{mem, str};

struct Entry {
	irq: u8,
	vector: u8,
	object: Arc<Interrupt>,
}

static LISTENERS: SpinLock<Arena<Entry, ()>> = Default::default();

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
			arch::amd64::idt_set(vector.into(), crate::wrap_idt!(handle_irq));
			io_apic::set_irq(irq, 0, vector, mode, false);
		}
		let mut l = LISTENERS.lock();
		let h = l.insert_with(|handle| Entry {
			irq,
			vector,
			object: Arc::new(Interrupt(
				InterruptInner {
					mode,
					irq,
					triggered: false,
					handle,
					wake: Default::default(),
				}
				.into(),
			)),
		});
		Ticket::new_complete(Ok(l[h].object.clone()))
	}
}

struct InterruptInner {
	mode: TriggerMode,
	irq: u8,
	triggered: bool,
	handle: arena::Handle<()>,
	wake: Vec<TicketWaker<Box<[u8]>>>,
}

struct Interrupt(SpinLock<InterruptInner>);

impl Drop for Interrupt {
	fn drop(&mut self) {
		let mut l = LISTENERS.auto_lock();
		let e = l.remove(self.0.get_mut().handle).unwrap();
		unsafe { arch::deallocate_irq(e.vector) }
	}
}

impl Object for Interrupt {
	fn read(self: Arc<Self>, _: usize) -> Ticket<Box<[u8]>> {
		let mut irq = self.0.lock();
		if mem::take(&mut irq.triggered) {
			Ticket::new_complete(Ok([].into()))
		} else {
			let (t, w) = Ticket::new();
			irq.wake.push(w);
			t
		}
	}

	fn write(self: Arc<Self>, _: &[u8]) -> Ticket<u64> {
		let mut irq = self.0.lock();
		// FIXME check if we actually need to send an EOI
		unsafe {
			if irq.mode == TriggerMode::Level {
				io_apic::mask_irq(irq.irq, false);
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

extern "C" fn handle_irq() {
	let mut l = LISTENERS.isr_lock();
	// TODO irq numbers
	for (_, e) in l.iter_mut() {
		let mut w = e.object.0.isr_lock();
		if let Some(w) = w.wake.pop() {
			w.isr_complete(Ok([].into()));
		} else {
			w.triggered = true;
		}
		if w.mode == TriggerMode::Level {
			unsafe { io_apic::mask_irq(e.irq, true) };
		}
	}
	local_apic::get().eoi.set(0);
}
