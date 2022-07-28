mod table;

use super::*;
use crate::{
	object_table::{Root, TicketWaker},
	sync::SpinLock,
	wrap_idt,
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::mem;
use scancodes::{
	scanset::ps2::{scanset2_decode, DecodeError},
	Event,
};

enum KeyboardCommand {
	#[allow(dead_code)]
	SetLed = 0xed,
	#[allow(dead_code)]
	Echo = 0xee,
	GetSetScanCodeSet = 0xf0,
}

static mut PORT: Port = Port::P1;

struct LossyRingBuffer {
	push: u8,
	pop: u8,
	data: [Event; 128],
}

impl LossyRingBuffer {
	fn push(&mut self, item: Event) {
		self.data[usize::from(self.push & 0x7f)] = item;
		let np = self.push.wrapping_add(1);
		if np ^ 128 != self.pop {
			self.push = np;
		}
	}

	fn pop(&mut self) -> Option<Event> {
		(self.pop != self.push).then(|| {
			let item = self.data[usize::from(self.pop & 0x7f)];
			self.pop = self.pop.wrapping_add(1);
			item
		})
	}
}

static EVENTS: SpinLock<LossyRingBuffer> = SpinLock::new(LossyRingBuffer {
	push: 0,
	pop: 0,
	data: [Default::default(); 128],
});
static SCANCODE_READERS: SpinLock<Vec<TicketWaker<Box<[u8]>>>> = SpinLock::new(Vec::new());

static mut INIT: bool = false;

pub(super) unsafe fn init(port: Port) {
	unsafe {
		// Use scancode set 2 since it's the only set that should be supported on all systems.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
		read_port_data_with_acknowledge().unwrap();
		write_raw_port_command(port, 2).unwrap();
		read_port_data_with_acknowledge().unwrap();

		// Just for sanity, ensure scancode set 2 is actually being used.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
		read_port_data_with_acknowledge().unwrap();
		write_raw_port_command(port, 0).unwrap();
		read_port_data_with_acknowledge().unwrap();
		assert_eq!(
			read_port_data_with_resend(),
			Ok(2),
			"scancode set 2 is not supported"
		);

		// Save port
		PORT = port;

		// Install an IRQ
		install_irq(port, wrap_idt!(handle_irq));

		// Enable scanning
		write_port_command(port, PortCommand::EnableScanning).unwrap();
		read_port_data_with_acknowledge().unwrap();

		INIT = true;
	}
}

pub(super) fn post_init(root: &Root) {
	if unsafe { INIT } {
		let tbl = Arc::new(table::KeyboardTable) as Arc<dyn crate::object_table::Object>;
		root.add(&b"ps2_keyboard"[..], Arc::downgrade(&tbl));
		mem::forget(tbl)
	}
}

extern "C" fn handle_irq() {
	static mut BUF: [u8; 8] = [0; 8];
	static mut INDEX: u8 = 0;

	let Ok(b) = (unsafe { read_port_data_nowait() }) else {
		// TODO for some reason the keyboard fires an IRQ for seemingly no reason. Just
		// ignore them for now.
		crate::driver::apic::local_apic::get().eoi.set(0);
		return;
	};
	// SAFETY: the IRQ handler cannot be interrupt nor won't it run from multiple threads.
	unsafe {
		BUF[usize::from(INDEX)] = b;
		INDEX += 1;
	}
	crate::driver::apic::local_apic::get().eoi.set(0);

	let seq = unsafe { &BUF[..INDEX.into()] };
	match scanset2_decode(seq) {
		Ok(code) => {
			unsafe { INDEX = 0 };
			if let Some(w) = SCANCODE_READERS.isr_lock().pop() {
				w.isr_complete(Ok(<[u8; 4]>::from(code).into()));
			} else {
				EVENTS.isr_lock().push(code);
			}
		}
		Err(DecodeError::Incomplete) => {}
		Err(DecodeError::NotRecognized) => {
			warn!("scancode {:x?} is not recognized", seq);
			unsafe { INDEX = 0 }
		}
	}
}
