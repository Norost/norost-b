use {
	super::*,
	alloc::collections::VecDeque,
	core::cell::{Cell, RefCell},
	driver_utils::os::stream_table::JobId,
	input::{Input, Movement, Type},
};

pub mod cmd {
	pub const SET_DEFAULTS: u8 = 0xf8;
	pub const DATA_ON: u8 = 0xf4;
}

#[derive(Default)]
pub struct Mouse {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<(Evt, u8)>>,
	buf: Cell<Buf>,
	buttons_pressed: Cell<u8>,
}

#[derive(Clone, Copy, Default)]
enum Evt {
	#[default]
	X,
	Y,
	BtnL,
	BtnR,
	BtnM,
}

impl From<Evt> for Type {
	fn from(d: Evt) -> Self {
		match d {
			Evt::X => Type::Relative(0, Movement::TranslationX),
			Evt::Y => Type::Relative(0, Movement::TranslationY),
			Evt::BtnL => Type::Button(0),
			Evt::BtnR => Type::Button(1),
			Evt::BtnM => Type::Button(2),
		}
	}
}

#[derive(Default)]
enum Buf {
	#[default]
	N0,
	N1,
	N2,
}

impl Mouse {
	fn add_event(&self, dir: Evt, lvl: u8, buf: &mut [u8; 8], pop: bool) -> Option<JobId> {
		if let Some(id) = pop.then(|| self.readers.borrow_mut().pop_front()).flatten() {
			Some(finish_job(id, buf, dir, lvl))
		} else {
			self.events.borrow_mut().push((dir, lvl));
			None
		}
	}
}

impl Device for Mouse {
	fn add_reader<'a>(&self, id: JobId, buf: &'a mut [u8; 8]) -> Option<JobId> {
		if let Some((k, l)) = self.events.borrow_mut().pop() {
			Some(finish_job(id, buf, k, l))
		} else {
			self.readers.borrow_mut().push_back(id);
			None
		}
	}

	fn handle_interrupt<'a>(&self, ps2: &mut Ps2, buf: &'a mut [u8; 8]) -> Option<JobId> {
		let x = ps2.read_port_data_nowait().unwrap();

		let mut id = None;
		self.buf.set(match self.buf.take() {
			Buf::N0 => {
				// Button presses.
				const BL: u8 = 1 << 0;
				const BR: u8 = 1 << 1;
				const BM: u8 = 1 << 2;
				let d = x ^ self.buttons_pressed.get();
				for (m, k) in [(BL, Evt::BtnL), (BR, Evt::BtnR), (BM, Evt::BtnM)] {
					if d & m != 0 {
						id = self.add_event(k, u8::from(x & m != 0) * 127, buf, id.is_none())
					}
				}
				self.buttons_pressed.set(x);
				Buf::N1
			}
			Buf::N1 => {
				// X movement
				id = self.add_event(Evt::X, x, buf, true);
				Buf::N2
			}
			Buf::N2 => {
				// Y movement
				id = self.add_event(Evt::Y, x, buf, true);
				Buf::N0
			}
		});
		id
	}
}

fn finish_job(id: JobId, buf: &mut [u8; 8], d: Evt, l: u8) -> JobId {
	buf.copy_from_slice(&u64::from(Input::new(d.into(), l as i8 as _)).to_le_bytes());
	id
}
