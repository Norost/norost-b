use super::*;
use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
use core::cell::RefCell;
use driver_utils::os::stream_table::JobId;
use scancodes::{
	scanset::ps2::{scanset2_decode, DecodeError},
	Event,
};

pub struct Keyboard {
	port: Port,
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<Event>>,
	buf: RefCell<TinyBuf>,
	interrupt: rt::Object,
}

enum KeyboardCommand {
	#[allow(dead_code)]
	SetLed = 0xed,
	#[allow(dead_code)]
	Echo = 0xee,
	GetSetScanCodeSet = 0xf0,
}

struct LossyRingBuffer<T> {
	push: u8,
	pop: u8,
	data: [T; 128],
}

#[derive(Default)]
struct TinyBuf {
	buf: [u8; 8],
	index: u8,
}

impl<T: ~const Default> const Default for LossyRingBuffer<T> {
	fn default() -> Self {
		Self {
			push: 0,
			pop: 0,
			data: [const { Default::default() }; 128],
		}
	}
}

impl<T: Copy> LossyRingBuffer<T> {
	fn push(&mut self, item: T) {
		self.data[usize::from(self.push & 0x7f)] = item;
		let np = self.push.wrapping_add(1);
		if np ^ 128 != self.pop {
			self.push = np;
		}
	}

	fn pop(&mut self) -> Option<T> {
		(self.pop != self.push).then(|| {
			let item = self.data[usize::from(self.pop & 0x7f)];
			self.pop = self.pop.wrapping_add(1);
			item
		})
	}
}

impl Keyboard {
	pub fn init(ps2: &mut Ps2, port: Port) -> Self {
		// Use scancode set 2 since it's the only set that should be supported on all systems.
		ps2.write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8)
			.unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();
		ps2.write_raw_port_command(port, 2).unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();

		// Just for sanity, ensure scancode set 2 is actually being used.
		ps2.write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8)
			.unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();
		ps2.write_raw_port_command(port, 0).unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();
		assert_eq!(
			ps2.read_port_data_with_resend(),
			Ok(2),
			"scancode set 2 is not supported"
		);

		// Install an IRQ
		let interrupt = ps2.install_interrupt(port);

		// Enable scanning
		ps2.write_port_command(port, PortCommand::EnableScanning)
			.unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();

		Self {
			port,
			events: Default::default(),
			readers: Default::default(),
			buf: Default::default(),
			interrupt,
		}
	}
}

impl Device for Keyboard {
	fn interrupter(&self) -> rt::RefObject<'_> {
		(&self.interrupt).into()
	}

	fn add_reader<'a>(&self, reader: JobId, buf: &'a mut [u8; 16]) -> Option<(JobId, &'a [u8])> {
		if let Some(e) = self.events.borrow_mut().pop() {
			let buf = &mut buf[..4];
			buf.copy_from_slice(&<[u8; 4]>::from(e));
			Some((reader, buf))
		} else {
			self.readers.borrow_mut().push_back(reader);
			None
		}
	}

	fn handle_interrupt<'a>(
		&self,
		ps2: &mut Ps2,
		out_buf: &'a mut [u8; 16],
	) -> Option<(JobId, &'a [u8])> {
		let Ok(b) = ps2.read_port_data_nowait() else {
			// TODO for some reason the keyboard fires an IRQ for seemingly no reason. Just
			// ignore them for now.
			return None;
		};
		let mut buf = self.buf.borrow_mut();
		let buf = &mut *buf;
		buf.buf[usize::from(buf.index)] = b;
		buf.index += 1;

		let seq = &buf.buf[..buf.index.into()];
		match scanset2_decode(seq) {
			Ok(code) => {
				buf.index = 0;
				if let Some(id) = self.readers.borrow_mut().pop_front() {
					let out_buf = &mut out_buf[..4];
					out_buf.copy_from_slice(&<[u8; 4]>::from(code));
					Some((id, out_buf))
				} else {
					self.events.borrow_mut().push(code);
					None
				}
			}
			Err(DecodeError::Incomplete) => None,
			Err(DecodeError::NotRecognized) => {
				log!("scancode {:x?} is not recognized", seq);
				buf.index = 0;
				None
			}
		}
	}
}
