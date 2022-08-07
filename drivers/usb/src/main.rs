//! # USB driver

#![no_std]
#![feature(start)]
#![feature(inline_const)]
#![feature(array_chunks)]
#![feature(alloc_layout_extra)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(result_option_inspect)]
#![feature(closure_lifetime_binder)]

extern crate alloc;

mod config;
mod dma;
mod loader;
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
	let conf = config::parse(&file_root.open(b"drivers/usb.scf").unwrap());

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
	let mut load_driver = BTreeMap::<u64, LoadDriver>::default();

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
						let d = devs
							.entry(d.slot())
							.and_modify(|_| panic!("slot already occupied"))
							.or_insert(d);
						let buf = dma::Dma::new_slice(64).unwrap_or_else(|_| todo!());
						let e = d
							.send_request(
								&mut ctrl,
								0,
								requests::Request::GetDescriptor {
									buffer: &buf,
									ty: requests::GetDescriptor::Device,
								},
							)
							.unwrap_or_else(|_| todo!());
						load_driver.insert(e, LoadDriver { buf, base: None });
					} else {
						todo!()
					}
				}
				Event::Transfer { id, slot } => {
					let dev = devs.get_mut(&slot).unwrap();
					if let Some(j) = jobs.remove(&id) {
						if let Some((job_id, resp)) = j.progress(&mut jobs, &mut ctrl, dev, &tbl) {
							tbl.enqueue(job_id, resp);
							tbl.flush();
						}
					} else if let Some(mut j) = load_driver.remove(&id) {
						let b = unsafe { j.buf.as_ref() };
						let mut it = requests::decode(b);
						match it.next().unwrap() {
							requests::DescriptorResult::Device(info) => {
								j.base = Some((info.class, info.subclass, info.protocol));
								if info.class == 0 && info.subclass == 0 {
									let e = dev
										.send_request(
											&mut ctrl,
											0,
											requests::Request::GetDescriptor {
												buffer: &j.buf,
												ty: requests::GetDescriptor::Configuration {
													index: 0,
												},
											},
										)
										.unwrap_or_else(|_| todo!());
									load_driver.insert(e, j);
								} else {
									// TODO
								}
							}
							requests::DescriptorResult::Configuration(info) => {
								let base = j.base.unwrap();
								let mut n = usize::from(info.num_interfaces);
								while n > 0 {
									match it.next().unwrap() {
										requests::DescriptorResult::Interface(i) => {
											let intf = (i.class, i.subclass, i.protocol);
											let driver =
												conf.get_driver(base, intf).expect("no driver");
											use rt::process::Process;
											let proc = Process::new_by_name(
												driver,
												Process::default_handles(),
												None::<&[u8]>.into_iter(),
												rt::args::Env::new(),
											)
											.expect("yay");
											n += usize::from(i.num_endpoints);
										}
										requests::DescriptorResult::Endpoint(e) => {
											rt::dbg!(e);
										}
										requests::DescriptorResult::Unknown { ty, .. } => {
											rt::dbg!(ty);
											n += 1;
										}
										requests::DescriptorResult::Invalid => {
											todo!("invalid descr")
										}
										requests::DescriptorResult::Truncated { length } => {
											todo!("fetch more ({})", length)
										}
										_ => todo!("unexpected"),
									}
									n -= 1;
								}
							}
							_ => unreachable!(),
						}
					} else {
						unreachable!()
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
				let info = requests::DescriptorResult::decode(b).into_device().unwrap();
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
				let s = requests::DescriptorResult::decode(b).into_string().unwrap();
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

struct LoadDriver {
	base: Option<(u8, u8, u8)>,
	buf: dma::Dma<[u8]>,
}
