use {
	super::*,
	alloc::collections::VecDeque,
	core::cell::{Cell, RefCell},
	driver_utils::os::stream_table::JobId,
	scancodes::{Event, KeyCode, SpecialKeyCode},
};

pub mod cmd {
	pub const SET_DEFAULTS: u8 = 0xf8;
	pub const DATA_ON: u8 = 0xf4;
}

pub struct Mouse {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<(Dir, u8)>>,
	buf: Cell<Buf>,
	send_empty: Cell<Option<Dir>>,
}

#[derive(Clone, Copy, Default)]
enum Dir {
	#[default]
	X,
	Y,
}

impl From<Dir> for KeyCode {
	fn from(d: Dir) -> Self {
		KeyCode::Special(match d {
			Dir::X => SpecialKeyCode::MouseX,
			Dir::Y => SpecialKeyCode::MouseY,
		})
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
	pub fn new() -> Self {
		Self {
			events: Default::default(),
			readers: Default::default(),
			buf: Default::default(),
			send_empty: Default::default(),
		}
	}

	fn add_event(&self, dir: Dir, lvl: u8, buf: &mut [u8; 4]) -> Option<JobId> {
		if let Some(id) = self.readers.borrow_mut().pop_front() {
			Some(finish_job(id, buf, dir, lvl))
		} else {
			self.events.borrow_mut().push((dir, lvl));
			None
		}
	}
}

impl Device for Mouse {
	fn add_reader<'a>(&self, id: JobId, buf: &'a mut [u8; 4]) -> Option<JobId> {
		if let Some(k) = self.send_empty.take() {
			Some(finish_job(id, buf, k, 0))
		} else if let Some((k, l)) = self.events.borrow_mut().pop() {
			Some(finish_job(id, buf, k, l))
		} else {
			self.readers.borrow_mut().push_back(id);
			None
		}
	}

	fn handle_interrupt<'a>(&self, ps2: &mut Ps2, buf: &'a mut [u8; 4]) -> Option<JobId> {
		let x = ps2.read_port_data_nowait().unwrap();

		let mut id = None;
		self.buf.set(match self.buf.take() {
			Buf::N0 => {
				// Button presses.
				Buf::N1
			}
			Buf::N1 => {
				// X movement
				id = self.add_event(Dir::X, x, buf);
				Buf::N2
			}
			Buf::N2 => {
				// Y movement
				id = self.add_event(Dir::Y, x, buf);
				Buf::N0
			}
		});
		id
	}
}

fn finish_job(id: JobId, buf: &mut [u8; 4], d: Dir, l: u8) -> JobId {
	buf.copy_from_slice(&u32::from(Event::new(d.into(), l as i8 as i16)).to_le_bytes());
	id
}
