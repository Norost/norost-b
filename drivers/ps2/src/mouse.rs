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
	events: RefCell<LossyRingBuffer<Input>>,
	buf: Cell<Buf>,
	buttons_pressed: Cell<u8>,
}

#[derive(Default)]
enum Buf {
	#[default]
	N0,
	N1,
	N2,
}

impl Mouse {
	fn add_input(&self, inp: Input, buf: &mut [u8; 8], pop: bool) -> Option<JobId> {
		if let Some(id) = pop.then(|| self.readers.borrow_mut().pop_front()).flatten() {
			Some(finish_job(id, buf, inp))
		} else {
			self.events.borrow_mut().push(inp);
			None
		}
	}
}

impl Device for Mouse {
	fn add_reader<'a>(&self, id: JobId, buf: &'a mut [u8; 8]) -> Option<JobId> {
		if let Some(inp) = self.events.borrow_mut().pop() {
			Some(finish_job(id, buf, inp))
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
				for i in 0..2 {
					let m = 1 << i;
					if d & m != 0 {
						let inp = Input::new(Type::Button(i), i32::from(x & m != 0) * i32::MAX);
						id = self.add_input(inp, buf, id.is_none())
					}
				}
				self.buttons_pressed.set(x);
				Buf::N1
			}
			Buf::N1 => {
				// X movement
				let inp = Input::new(Type::Relative(0, Movement::TranslationX), x as i8 as i32);
				id = self.add_input(inp, buf, true);
				Buf::N2
			}
			Buf::N2 => {
				// Y movement
				let inp = Input::new(Type::Relative(0, Movement::TranslationY), x as i8 as i32);
				id = self.add_input(inp, buf, true);
				Buf::N0
			}
		});
		id
	}
}

fn finish_job(id: JobId, buf: &mut [u8; 8], inp: Input) -> JobId {
	buf.copy_from_slice(&u64::from(inp).to_le_bytes());
	id
}
