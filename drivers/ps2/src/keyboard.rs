// https://www.win.tue.nl/~aeb/linux/kbd/scancodes-1.html
// https://web.archive.org/web/20030621203107/http://www.microsoft.com/whdc/hwdev/tech/input/Scancode.mspx
// https://web.archive.org/web/20030701121507/http://microsoft.com/hwdev/download/tech/input/translate.pdf

mod scanset2;

use super::*;
use alloc::collections::VecDeque;
use core::cell::{Cell, RefCell};
use driver_utils::os::stream_table::JobId;
use scancodes::{
	config::{Config, Modifiers},
	Event, KeyCode, SpecialKeyCode,
};

pub struct Keyboard {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<Event>>,
	interrupt: rt::Object,
	config: Config,
	translator: RefCell<scanset2::Translator>,
	modifiers: Cell<u8>,
}

const MOD_LSHIFT: u8 = 1 << 0;
const MOD_RSHIFT: u8 = 1 << 1;
const MOD_ALTGR: u8 = 1 << 2;
const MOD_CAPS: u8 = 1 << 3;
const APPLY_CAPS: u8 = MOD_LSHIFT | MOD_RSHIFT | MOD_CAPS;

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
				.open(b"drivers/keyboard.scf")
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

		rt::dbg!(&config);

		Self {
			events: Default::default(),
			readers: Default::default(),
			interrupt,
			config,
			translator: Default::default(),
			modifiers: 0.into(),
		}
	}

	fn toggle_modifier(&self, event: Event) {
		use {Event::*, KeyCode::*, SpecialKeyCode::*};
		let mut m = self.modifiers.get();
		match event {
			Press(Special(LeftShift)) => m |= MOD_LSHIFT,
			Press(Special(RightShift)) => m |= MOD_RSHIFT,
			Press(Special(AltGr)) => m |= MOD_ALTGR,
			Release(Special(LeftShift)) => m &= !MOD_LSHIFT,
			Release(Special(RightShift)) => m &= !MOD_RSHIFT,
			Release(Special(AltGr)) => m &= !MOD_ALTGR,
			Press(Special(CapsLock)) => m ^= MOD_CAPS,
			_ => {}
		}
		self.modifiers.set(m);
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

		let mut buf = [0; 4];
		let mut tr = self.translator.borrow_mut();
		let (release, seq) = tr.push(b, &mut buf)?;

		let Some(code) = self.config.raw(seq) else {
			log!("unknown HID sequence {:02x?}", seq);
			return None;
		};
		let m = self.modifiers.get();
		let code = self.config.modified(
			code,
			Modifiers {
				altgr: m & MOD_ALTGR != 0,
				caps: m & APPLY_CAPS != m & MOD_CAPS,
				num: false,
			},
		)?;
		let code = match release {
			true => Event::Release(code),
			false => Event::Press(code),
		};
		self.toggle_modifier(code);
		if let Some(id) = self.readers.borrow_mut().pop_front() {
			let out_buf = &mut out_buf[..4];
			out_buf.copy_from_slice(&u32::from(code).to_le_bytes());
			Some((id, out_buf))
		} else {
			self.events.borrow_mut().push(code);
			None
		}
	}
}
