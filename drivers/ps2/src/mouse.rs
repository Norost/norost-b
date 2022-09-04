use {
	super::*,
	alloc::collections::VecDeque,
	core::cell::{Cell, RefCell},
	driver_utils::os::stream_table::JobId,
	scancodes::{
		config::{Config, Modifiers},
		Event, KeyCode, SpecialKeyCode,
	},
};

pub struct Mouse {
	readers: RefCell<VecDeque<JobId>>,
	events: RefCell<LossyRingBuffer<[u8; 2]>>,
	interrupt: rt::Object,
	buf: Cell<Buf>,
}

#[derive(Default)]
enum Buf {
	#[default]
	N0,
	N1(u8),
	N2(u8, u8),
}

impl Mouse {
	pub fn init(ps2: &mut Ps2, port: Port) -> Self {
		// Set defaults
		ps2.write_raw_port_command(port, 0xf6).unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();

		// Install an IRQ
		let interrupt = ps2.install_interrupt(port);

		// Enable scanning
		ps2.write_port_command(port, PortCommand::EnableScanning)
			.unwrap();
		ps2.read_port_data_with_acknowledge().unwrap();

		Self {
			events: Default::default(),
			readers: Default::default(),
			interrupt,
			buf: Default::default(),
		}
	}
}

impl Device for Mouse {
	fn interrupter(&self) -> rt::RefObject<'_> {
		(&self.interrupt).into()
	}

	fn add_reader<'a>(&self, id: JobId, out_buf: &'a mut [u8; 16]) -> Option<(JobId, &'a [u8])> {
		if let Some([p, q]) = self.events.borrow_mut().pop() {
			Some(finish_job(id, out_buf, p, q))
		} else {
			self.readers.borrow_mut().push_back(id);
			None
		}
	}

	fn handle_interrupt<'a>(
		&self,
		ps2: &mut Ps2,
		out_buf: &'a mut [u8; 16],
	) -> Option<(JobId, &'a [u8])> {
		let Ok(x) = ps2.read_port_data_nowait() else {
			// TODO for some reason the mouse fires an IRQ for seemingly no reason. Just
			// ignore them for now.
			return None;
		};

		self.buf.set(match self.buf.take() {
			Buf::N0 => Buf::N1(x),
			Buf::N1(p) => Buf::N2(p, x),
			Buf::N2(p, q) => {
				return if let Some(id) = self.readers.borrow_mut().pop_front() {
					Some(finish_job(id, out_buf, q, x))
				} else {
					self.events.borrow_mut().push([q, x]);
					None
				};
			}
		});
		None
	}
}

fn finish_job(id: JobId, buf: &mut [u8; 16], p: u8, q: u8) -> (JobId, &[u8]) {
	let [a, b] = i16::from(p as i8).to_le_bytes();
	let [c, d] = i16::from(q as i8).to_le_bytes();
	let buf = &mut buf[..4];
	buf.copy_from_slice(&[a, b, c, d]);
	(id, buf)
}
