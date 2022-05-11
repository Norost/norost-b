use super::*;
use crate::{object_table::Root, sync::SpinLock, wrap_idt};
use scancodes::{
	scanset::ps2::{scanset2_decode, DecodeError},
	Event, ScanCode,
};

enum KeyboardCommand {
	SetLed = 0xed,
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

pub(super) unsafe fn init(port: Port, root: &Root) {
	unsafe {
		// Use scancode set 2 since it's the only set that should be supported on all systems.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
		write_raw_port_command(port, 2).unwrap();
		read_port_data_with_acknowledge().unwrap();

		// Just for sanity, ensure scancode set 2 is actually being used.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
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
		install_irq(port, wrap_idt!(int handle_irq));

		// Enable scanning
		write_port_command(port, PortCommand::EnableScanning).unwrap();
		read_port_data_with_acknowledge().unwrap();
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
			dbg!(code);
			unsafe { INDEX = 0 };
			EVENTS.isr_lock().push(code);
		}
		Err(DecodeError::Incomplete) => {}
		Err(DecodeError::NotRecognized) => {
			warn!("scancode {:x?} is not recognized", seq);
			unsafe { INDEX = 0 }
		}
	}
}
