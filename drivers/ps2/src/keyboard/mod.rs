// https://www.win.tue.nl/~aeb/linux/kbd/scancodes-1.html
// https://web.archive.org/web/20030621203107/http://www.microsoft.com/whdc/hwdev/tech/input/Scancode.mspx
// https://web.archive.org/web/20030701121507/http://microsoft.com/hwdev/download/tech/input/translate.pdf

mod scanset2;

use {
	super::*,
	alloc::collections::VecDeque,
	core::cell::{Cell, RefCell},
	driver_utils::os::stream_table::JobId,
	input::{
		config::{Config, Modifiers},
		Input, Keyboard as Kbd, Type,
	},
};

pub mod cmd {
	pub const GET_SET_SCANCODE_SET: u8 = 0xf0;
}

pub struct Keyboard {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<Input>>,
	config: Config,
	translator: RefCell<scanset2::Translator>,
	modifiers: Cell<u8>,
}

const MOD_LSHIFT: u8 = 1 << 0;
const MOD_RSHIFT: u8 = 1 << 1;
const MOD_ALTGR: u8 = 1 << 2;
const MOD_CAPS: u8 = 1 << 3;
const APPLY_CAPS: u8 = MOD_LSHIFT | MOD_RSHIFT | MOD_CAPS;

impl Keyboard {
	pub fn new() -> Self {
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
			input::config::parse(&buf).expect("failed to parse config")
		};

		Self {
			events: Default::default(),
			readers: Default::default(),
			config,
			translator: Default::default(),
			modifiers: 0.into(),
		}
	}

	fn toggle_modifier(&self, input: Input) {
		use {Kbd::*, Type::Keyboard as K};
		let mut m = self.modifiers.get();
		match (input.is_press(), input.ty) {
			(true, K(LeftShift)) => m |= MOD_LSHIFT,
			(true, K(RightShift)) => m |= MOD_RSHIFT,
			(true, K(AltGr)) => m |= MOD_ALTGR,
			(false, K(LeftShift)) => m &= !MOD_LSHIFT,
			(false, K(RightShift)) => m &= !MOD_RSHIFT,
			(false, K(AltGr)) => m &= !MOD_ALTGR,
			(true, K(CapsLock)) => m ^= MOD_CAPS,
			_ => {}
		}
		self.modifiers.set(m);
	}
}

impl Device for Keyboard {
	fn add_reader<'a>(&self, reader: JobId, buf: &'a mut [u8; 8]) -> Option<JobId> {
		if let Some(e) = self.events.borrow_mut().pop() {
			buf.copy_from_slice(&u64::from(e).to_le_bytes());
			Some(reader)
		} else {
			self.readers.borrow_mut().push_back(reader);
			None
		}
	}

	fn handle_interrupt<'a>(&self, ps2: &mut Ps2, buf: &'a mut [u8; 8]) -> Option<JobId> {
		let b = ps2.read_port_data_nowait().unwrap();
		let mut tr = self.translator.borrow_mut();
		let (release, seq) = tr.push(b, buf)?;

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
		);
		let code = Input::new(code, i32::from(!release) * i32::MAX);
		self.toggle_modifier(code);
		if let Some(id) = self.readers.borrow_mut().pop_front() {
			buf.copy_from_slice(&u64::from(code).to_le_bytes());
			Some(id)
		} else {
			self.events.borrow_mut().push(code);
			None
		}
	}
}
