use super::*;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use driver_utils::os::stream_table::JobId;
use scancodes::{config::Config, Event};

pub struct Keyboard {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<Event>>,
	buf: RefCell<TinyBuf>,
	interrupt: rt::Object,
	config: Config,
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

impl<T: Default + Copy> Default for LossyRingBuffer<T> {
	fn default() -> Self {
		Self {
			push: 0,
			pop: 0,
			data: [Default::default(); 128],
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
		let config = {
			let f = rt::io::file_root()
				.unwrap()
				.open(b"drivers/scanset2.scf")
				.unwrap();
			let len = f
				.seek(rt::io::SeekFrom::End(0))
				.unwrap()
				.try_into()
				.unwrap();
			f.seek(rt::io::SeekFrom::Start(0)).unwrap();
			let mut buf = alloc::vec::Vec::with_capacity(len);
			let mut offt = 0;
			while offt < len {
				offt += f
					.read_uninit(&mut buf.spare_capacity_mut()[offt..])
					.unwrap()
					.0
					.len();
			}
			unsafe { buf.set_len(len) };
			scancodes::config::parse(&buf).expect("failed to parse config")
		};

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
			events: Default::default(),
			readers: Default::default(),
			buf: Default::default(),
			interrupt,
			config,
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
			buf.copy_from_slice(&u32::from(e).to_le_bytes());
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

		let mut seq = &buf.buf[..buf.index.into()];
		let mut release = seq[0] == 0xf0;
		if release {
			seq = &seq[1..];
		}
		if let Some(code) = self.config.map_raw(seq) {
			let code = if release {
				Event::Release(code)
			} else {
				Event::Press(code)
			};
			buf.index = 0;
			if let Some(id) = self.readers.borrow_mut().pop_front() {
				let out_buf = &mut out_buf[..4];
				out_buf.copy_from_slice(&u32::from(code).to_le_bytes());
				Some((id, out_buf))
			} else {
				self.events.borrow_mut().push(code);
				None
			}
		} else {
			None
		}
	}
}
