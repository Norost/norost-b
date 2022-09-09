use {
	alloc::{boxed::Box, collections::BTreeMap, string::ToString, vec::Vec},
	core::{
		future::Future,
		num::{NonZeroU8, Wrapping},
		pin::Pin,
		task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
	},
	io_queue_rt::{Open, Queue, Read, Write},
	usb_request::descriptor::{Direction, Endpoint, EndpointNumber, EndpointTransfer},
};

const MSG_SIZE: usize = 32;

pub struct Drivers<'a> {
	queue: &'a Queue,
	drivers: BTreeMap<NonZeroU8, DeviceDriver<'a>>,
	handlers: BTreeMap<Box<str>, rt::Object>,
}

impl<'a> Drivers<'a> {
	pub fn new(queue: &'a Queue) -> Self {
		Self { queue, drivers: Default::default(), handlers: Default::default() }
	}

	pub fn dequeue(&mut self) -> Option<(NonZeroU8, u32, Event)> {
		let wk = self.make_waker();
		let mut cx = Context::from_waker(&wk);
		for (&slot, driver) in self.drivers.iter_mut() {
			// Remove done tasks
			for i in (0..driver.write_tasks.len()).rev() {
				if Pin::new(&mut driver.write_tasks[i])
					.poll(&mut cx)
					.is_ready()
				{
					driver.write_tasks.swap_remove(i);
				}
			}

			if let Some(share) = driver.share_task.as_mut() {
				if let Poll::Ready((res, ())) = Pin::new(share).poll(&mut cx) {
					let obj = rt::Object::from_raw(res.unwrap());
					self.handlers.insert(
						gen_name(&driver.name, |s| self.handlers.contains_key(s)),
						obj,
					);
					driver.share_task = None;
				}
			} else if let Poll::Ready((res, buf)) = Pin::new(&mut driver.read_task).poll(&mut cx) {
				res.unwrap();
				let evt = match *buf.get(0).unwrap_or_else(|| todo!("no msg")) {
					ipc_usb::SEND_TY_PUBLIC_OBJECT => {
						assert!(driver.share_task.is_none(), "share already in progress");
						driver.share_task = Some(
							self.queue
								.submit_open(driver.stdout.as_raw(), ())
								.unwrap_or_else(|e| todo!("{:?}", e)),
						);
						None
					}
					ipc_usb::SEND_TY_DATA_IN => {
						let [_, endpoint, a, b, c, d]: [u8; 6] =
							(&*buf).try_into().expect("invalid msg");
						Some(Event::DataIn { endpoint, size: u32::from_le_bytes([a, b, c, d]) })
					}
					ipc_usb::SEND_TY_DATA_OUT => {
						let [_, endpoint]: [u8; 2] = (&*buf).try_into().expect("invalid msg");
						let mut buf = [0; 1024 + 32];
						let l = driver.stdout.read(&mut buf).unwrap();
						let mut data = crate::dma::Dma::new_slice(l).unwrap();
						unsafe { data.as_mut().copy_from_slice(&buf[..l]) }
						Some(Event::DataOut { endpoint, data })
					}
					ipc_usb::SEND_TY_GET_DESCRIPTOR => {
						let [_, recp, ty, index, a, b]: [u8; 6] = (&*buf).try_into().unwrap();
						let len = u16::from_le_bytes([a, b]);
						let recipient = match recp {
							0 => Recipient::Device,
							1 => Recipient::Interface,
							_ => todo!(),
						};
						Some(Event::GetDescriptor { recipient, ty, index, len })
					}
					_ => todo!(
						"invalid msg: {:?}",
						alloc::string::String::from_utf8_lossy(&buf)
					),
				};
				driver.read_task = read(&self.queue, &driver.stdout, buf);
				if let Some(evt) = evt {
					let id = driver.msg_in_counter;
					driver.msg_in_counter += 1;
					return Some((slot, id.0, evt));
				}
			}
		}
		None
	}

	pub fn load_driver(
		&mut self,
		slot: NonZeroU8,
		driver: &crate::config::Driver,
		base: (u8, u8, u8),
		interface: (u8, u8, u8),
		endpoints: &[Endpoint],
	) -> rt::io::Result<()> {
		trace!("load driver for {}", slot);
		self.drivers
			.entry(slot)
			.and_modify(|_| panic!("driver already present in slot {}", slot))
			.or_insert(DeviceDriver::new(
				driver, self.queue, base, interface, endpoints,
			)?);
		Ok(())
	}

	pub fn send(&mut self, slot: NonZeroU8, msg: Message<'_>) -> rt::io::Result<()> {
		let d = self.drivers.get_mut(&slot).expect("no driver at slot");
		let mut v = Vec::new();
		match msg {
			Message::DataIn { endpoint, data } => {
				v.push(ipc_usb::RECV_TY_DATA_IN);
				v.push(endpoint);
				v.extend_from_slice(data);
			}
			Message::Error { id, code, message } => {
				v.push(ipc_usb::RECV_TY_ERROR);
				v.extend_from_slice(&id.to_le_bytes());
				v.push(code);
				v.extend_from_slice(message.as_ref());
			}
		}
		let wr = write(self.queue, &d.stdin, v);
		d.write_tasks.push(wr);
		d.msg_out_counter += 1;
		Ok(())
	}

	pub fn handler<'h>(&'h self, name: &str) -> Option<rt::RefObject<'h>> {
		self.handlers.get(name).map(Into::into)
	}

	pub fn handler_at(&self, index: usize) -> Option<(&str, &rt::Object)> {
		self.handlers
			.iter()
			.skip(index)
			.next()
			.map(|(k, v)| (&**k, v))
	}

	fn make_waker(&self) -> Waker {
		fn clone(p: *const ()) -> RawWaker {
			RawWaker::new(p, &VTABLE)
		}
		fn wake(_: *const ()) {}
		fn wake_by_ref(_: *const ()) {}
		fn drop(_: *const ()) {}

		static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
		let wk = RawWaker::new(self as *const _ as _, &VTABLE);
		unsafe { Waker::from_raw(wk) }
	}
}

struct DeviceDriver<'a> {
	#[allow(dead_code)]
	process: rt::Process,
	stdin: rt::Object,
	stdout: rt::Object,
	read_task: Read<'a, Vec<u8>>,
	write_tasks: Vec<Write<'a, Vec<u8>>>,
	share_task: Option<Open<'a, ()>>,
	name: Box<str>,
	msg_in_counter: Wrapping<u32>,
	msg_out_counter: Wrapping<u32>,
}

impl<'a> DeviceDriver<'a> {
	fn new(
		driver: &crate::config::Driver,
		queue: &'a Queue,
		base: (u8, u8, u8),
		interface: (u8, u8, u8),
		endpoints: &[Endpoint],
	) -> rt::io::Result<Self> {
		let (stdin, proc_stdin) = rt::Object::new(rt::NewObject::MessagePipe)?;
		let (proc_stdout, stdout) = rt::Object::new(rt::NewObject::MessagePipe)?;

		let mut arg = [0; 2 * 6 + 5];
		let ((a, b, c), (d, e, f)) = (base, interface);
		for (i, n) in [a, b, c, d, e, f].into_iter().enumerate() {
			let f = |d| {
				d + match d {
					0..=9 => b'0',
					10..=15 => b'a',
					_ => unreachable!(),
				}
			};
			arg[0 + i * 3] = f(n / 16);
			arg[1 + i * 3] = f(n % 16);
			arg.get_mut(2 + i * 3).map(|c| *c = b',');
		}

		let mut p = rt::process::Builder::new()?;
		p.set_binary_by_name(driver.path.as_bytes())?;
		p.add_args([driver.path.as_bytes(), b"--class", &arg])?;

		for ep in endpoints {
			let num = match ep.address.number() {
				EndpointNumber::N1 => "1",
				EndpointNumber::N2 => "2",
				EndpointNumber::N3 => "3",
				EndpointNumber::N4 => "4",
				EndpointNumber::N5 => "5",
				EndpointNumber::N6 => "6",
				EndpointNumber::N7 => "7",
				EndpointNumber::N8 => "8",
				EndpointNumber::N9 => "9",
				EndpointNumber::N10 => "10",
				EndpointNumber::N11 => "11",
				EndpointNumber::N12 => "12",
				EndpointNumber::N13 => "13",
				EndpointNumber::N14 => "14",
				EndpointNumber::N15 => "15",
			};
			let arg = match (ep.attributes.transfer(), ep.address.direction()) {
				(EndpointTransfer::Bulk, Direction::Out) => "--bulk-out",
				(EndpointTransfer::Bulk, Direction::In) => "--bulk-in",
				(EndpointTransfer::Interrupt, Direction::Out) => "--itnr-out",
				(EndpointTransfer::Interrupt, Direction::In) => "--intr-in",
				(EndpointTransfer::Isoch, Direction::Out) => "--isoch-out",
				(EndpointTransfer::Isoch, Direction::In) => "--isoch-in",
				(EndpointTransfer::Control, _) => unreachable!(),
			};
			p.add_args([arg, num])?;
		}

		p.add_object(b"in", &proc_stdin)?;
		p.add_object(b"out", &proc_stdout)?;
		rt::io::stderr()
			.map(|o| p.add_object(b"err", &o))
			.transpose()?;
		rt::io::file_root()
			.map(|o| p.add_object(b"file", &o))
			.transpose()?;

		let process = p.spawn()?;
		let read_task = read(queue, &stdout, Vec::with_capacity(MSG_SIZE));

		Ok(Self {
			process,
			stdin,
			stdout,
			read_task,
			write_tasks: Default::default(),
			share_task: Default::default(),
			name: driver.name.as_deref().unwrap_or("unnamed{n}").into(),
			msg_in_counter: Wrapping(0),
			msg_out_counter: Wrapping(0),
		})
	}
}

pub enum Event {
	DataIn { endpoint: u8, size: u32 },
	DataOut { endpoint: u8, data: crate::dma::Dma<[u8]> },
	GetDescriptor { recipient: Recipient, ty: u8, index: u8, len: u16 },
}

pub enum Recipient {
	Device,
	Interface,
}

pub enum Message<'a> {
	DataIn { endpoint: u8, data: &'a [u8] },
	Error { id: u32, code: u8, message: &'a str },
}

fn read<'a>(queue: &'a Queue, stdout: &rt::Object, mut buf: Vec<u8>) -> Read<'a, Vec<u8>> {
	buf.clear();
	queue
		.submit_read(stdout.as_raw(), buf)
		.unwrap_or_else(|e| todo!("{:?}", e))
}

fn write<'a>(queue: &'a Queue, stdin: &rt::Object, data: Vec<u8>) -> Write<'a, Vec<u8>> {
	queue
		.submit_write(stdin.as_raw(), data)
		.unwrap_or_else(|e| todo!("{:?}", e))
}

fn gen_name(template: &str, f: impl Fn(&str) -> bool) -> Box<str> {
	for i in 0usize.. {
		let s = template.replace("{n}", &i.to_string());
		assert!(&s != template, "name cannot be unique");
		if !f(&s) {
			return s.into();
		}
	}
	unreachable!()
}
