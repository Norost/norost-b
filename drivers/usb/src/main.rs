//! # USB driver

#![no_std]
#![feature(start)]
#![feature(inline_const)]
#![feature(array_chunks)]
#![feature(alloc_layout_extra)]
#![feature(nonnull_slice_from_raw_parts)]

extern crate alloc;

mod dma;
mod requests;
mod xhci;

use alloc::{collections::BTreeMap, vec::Vec};
use core::num::NonZeroU8;
use driver_utils::os::stream_table::{JobId, Request, Response, StreamTable};
use rt::{Error, Handle};
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let file_root = rt::io::file_root().expect("no file root");
	/*
	let table_name = rt::args::Args::new()
		.skip(1)
		.next()
		.expect("expected table name");
	*/

	let dev = {
		let s = b" 1b36:000d";
		let mut it = file_root.open(b"pci/info").unwrap();
		let mut buf = [0; 64];
		loop {
			let l = it.read(&mut buf).unwrap();
			assert!(l != 0, "device not found");
			let dev = &buf[..l];
			if dev.ends_with(s) {
				let mut path = Vec::from(*b"pci/");
				path.extend(&dev[..7]);
				break file_root.open(&path).unwrap();
			}
		}
	};

	use self::xhci::Event;
	let mut ctrl = xhci::Xhci::new(dev).unwrap();

	let mut init_dev_wait = Vec::new();
	let mut init_dev_alloc = Vec::<(_, xhci::device::AllocSlot)>::new();
	let mut init_dev_addr = Vec::<(_, xhci::device::SetAddress)>::new();

	let mut devs = BTreeMap::default();
	let mut jobs = BTreeMap::<u64, Job>::default();

	let (tbl_buf, tbl_buf_phys) =
		driver_utils::dma::alloc_dma_object((1 << 20).try_into().unwrap()).unwrap();
	let tbl = StreamTable::new(&tbl_buf, 512.try_into().unwrap(), (1 << 12) - 1);
	let tbl_get_phys = |data: ()| todo!();
	file_root
		.create(b"usb")
		.unwrap()
		.share(tbl.public())
		.unwrap();
	let mut objects = driver_utils::Arena::new();

	loop {
		if let Some(e) = ctrl.dequeue_event() {
			match e {
				Event::PortStatusChange { port } => {
					let wait = ctrl.init_device(port).unwrap();
					init_dev_wait.push(wait);
				}
				Event::CommandCompletion { id, slot } => {
					if let Some(i) = init_dev_alloc.iter().position(|(i, _)| *i == id) {
						let e = init_dev_alloc
							.swap_remove(i)
							.1
							.init(&mut ctrl, slot)
							.unwrap_or_else(|_| todo!());
						init_dev_addr.push(e);
					} else if let Some(i) = init_dev_addr.iter().position(|(i, _)| *i == id) {
						let d = init_dev_addr.swap_remove(i).1.finish();
						devs.entry(d.slot())
							.and_modify(|_| panic!("slot already occupied"))
							.or_insert(d);
					} else {
						todo!()
					}
				}
				Event::Transfer { id, slot } => {
					let dev = devs.get_mut(&slot).unwrap();
					if let Some((job_id, resp)) = jobs
						.remove(&id)
						.unwrap()
						.progress(&mut jobs, &mut ctrl, dev, &tbl)
					{
						tbl.enqueue(job_id, resp);
						tbl.flush();
					}
				}
			}
		}

		for i in (0..init_dev_wait.len()).rev() {
			if let Some(Ok(e)) = init_dev_wait[i].poll(&mut ctrl) {
				init_dev_alloc.push(e);
				init_dev_wait.swap_remove(i);
			}
		}

		#[derive(Debug)]
		enum Object {
			Root { i: u8 },
			List { slot: u8 },
		}

		'req: while let Some((handle, job_id, req)) = tbl.dequeue() {
			let mut buf = [0; 8];
			let resp = match req {
				Request::Open { path } => match (handle, &*path.copy_into(&mut buf).0) {
					(Handle::MAX, b"") => Response::Handle(objects.insert(Object::Root { i: 0 })),
					(Handle::MAX, b"list") | (Handle::MAX, b"list/") => {
						Response::Handle(objects.insert(Object::List { slot: 0 }))
					}
					(_, p) => todo!("{:?}", core::str::from_utf8(p)),
					_ => Response::Error(Error::DoesNotExist),
				},
				Request::Read { amount } => match &mut objects[handle] {
					Object::Root { i } => {
						let s: &[u8] = match i {
							0 => b"list",
							_ => {
								*i -= 1;
								b""
							}
						};
						*i += 1;
						let b = tbl.alloc(s.len()).expect("out of buffers");
						b.copy_from(0, s);
						Response::Data(b)
					}
					Object::List { slot } => loop {
						if *slot == 255 {
							break Response::Data(tbl.alloc(0).unwrap());
						} else {
							*slot += 1;
							let r: NonZeroU8 = (*slot).try_into().unwrap();
							if let Some((_, dev)) = devs.range_mut(r..).next() {
								Job::get_info(&mut jobs, &mut ctrl, dev, job_id);
								continue 'req;
							} else {
								*slot = 255;
							}
						}
					},
				},
				Request::Close => {
					objects.remove(handle);
					continue;
				}
				_ => Response::Error(Error::InvalidOperation),
			};
			tbl.enqueue(job_id, resp);
			tbl.flush();
		}

		rt::thread::sleep(core::time::Duration::from_millis(10));
	}
}

struct Job {
	state: JobState,
	job_id: JobId,
	buf: dma::Dma<[u8]>,
}

enum JobState {
	WaitDeviceInfo,
	WaitDeviceName { info: requests::Device },
}

impl Job {
	fn get_info(
		jobs: &mut BTreeMap<u64, Self>,
		ctrl: &mut xhci::Xhci,
		dev: &mut xhci::device::Device,
		job_id: JobId,
	) {
		let buf = dma::Dma::new_slice(64).unwrap();
		let id = dev
			.send_request(
				ctrl,
				0,
				requests::Request::GetDescriptor {
					buffer: &buf,
					ty: requests::GetDescriptor::Device,
				},
			)
			.unwrap_or_else(|_| todo!());
		jobs.insert(
			id,
			Self {
				state: JobState::WaitDeviceInfo,
				job_id,
				buf,
			},
		);
	}

	fn progress<'a>(
		mut self,
		jobs: &mut BTreeMap<u64, Self>,
		ctrl: &mut xhci::Xhci,
		dev: &mut xhci::device::Device,
		tbl: &'a StreamTable,
	) -> Option<(JobId, Response<'a>)> {
		match &self.state {
			JobState::WaitDeviceInfo => {
				let b = unsafe { self.buf.as_ref() };
				let info = requests::DescriptorResult::decode(b)
					.unwrap_or_else(|_| todo!())
					.into_device()
					.unwrap();
				let id = dev
					.send_request(
						ctrl,
						0,
						requests::Request::GetDescriptor {
							buffer: &self.buf,
							ty: requests::GetDescriptor::String {
								index: info.index_product,
							},
						},
					)
					.unwrap_or_else(|_| todo!());
				self.state = JobState::WaitDeviceName { info };
				jobs.insert(id, self);
				None
			}
			JobState::WaitDeviceName { info: _ } => {
				let b = unsafe { self.buf.as_ref() };
				let s = requests::DescriptorResult::decode(b)
					.unwrap_or_else(|_| todo!())
					.into_string()
					.unwrap();
				let name = tbl.alloc(s.len()).expect("out of buffers");
				for (i, mut c) in s.enumerate() {
					if c > 127 {
						c = b'?' as _
					}
					name.copy_from(i, &[c as _]);
				}
				Some((self.job_id, Response::Data(name)))
			}
		}
	}
}
