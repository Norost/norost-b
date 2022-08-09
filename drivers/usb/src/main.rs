//! # USB driver

#![no_std]
#![feature(start)]
#![feature(inline_const, const_option)]
#![feature(let_else)]
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

	let mut ctrl = xhci::Xhci::new(dev).unwrap();

	let mut jobs = BTreeMap::<u64, Job>::default();
	let mut load_driver = BTreeMap::<u64, LoadDriver>::default();

	let mut conf_driver = BTreeMap::default();

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
		while let Some(e) = ctrl.poll() {
			use self::xhci::Event;
			match e {
				Event::NewDevice { slot } => {
					let e = ctrl
						.send_request(
							slot,
							requests::Request::GetDescriptor {
								buffer: dma::Dma::new_slice(64).unwrap_or_else(|_| todo!()),
								ty: requests::GetDescriptor::Device,
							},
						)
						.unwrap_or_else(|_| todo!());
					load_driver.insert(e, LoadDriver { base: None });
				}
				Event::Transfer {
					slot,
					id,
					buffer,
					code,
				} => {
					if let Some(j) = jobs.remove(&id) {
						if let Some((job_id, resp)) =
							j.progress(&mut jobs, &mut ctrl, slot, buffer.unwrap(), &tbl)
						{
							tbl.enqueue(job_id, resp);
							tbl.flush();
						}
					} else if let Some(mut j) = load_driver.remove(&id) {
						let buffer = buffer.unwrap();
						let mut it = requests::decode(unsafe { buffer.as_ref() });
						match it.next().unwrap() {
							requests::DescriptorResult::Device(info) => {
								j.base = Some((info.class, info.subclass, info.protocol));
								if info.class == 0 && info.subclass == 0 {
									let e = ctrl
										.send_request(
											slot,
											requests::Request::GetDescriptor {
												buffer,
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
							requests::DescriptorResult::Configuration(config) => {
								rt::dbg!(&config);
								let base = j.base.unwrap();
								let mut n = usize::from(config.num_interfaces);
								let mut driver = None;
								let mut endpoints = Vec::new();
								while n > 0 {
									match it.next().unwrap() {
										requests::DescriptorResult::Interface(i) => {
											rt::dbg!(&i);
											let intf = (i.class, i.subclass, i.protocol);
											if driver.is_none() {
												n += usize::from(i.num_endpoints);
												conf.get_driver(base, intf)
													.map(|d| driver = Some((d, i)));
											} else {
												break;
											}
										}
										requests::DescriptorResult::Endpoint(e) => {
											rt::dbg!(&e);
											if driver.is_some() {
												endpoints.push(e)
											}
										}
										requests::DescriptorResult::Unknown { ty, .. } => continue,
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

								let (driver, interface) = driver.expect("no driver");
								rt::dbg!();
								if driver == b"drivers/usb_kbd" {
									let id = ctrl
										.send_request(
											slot,
											requests::Request::SetConfiguration {
												value: config.index_configuration,
											},
										)
										.unwrap_or_else(|_| todo!());
									ctrl.configure_device(
										slot,
										xhci::DeviceConfig {
											config,
											interface,
											endpoints,
										},
									)
									.unwrap_or_else(|_| todo!());
									conf_driver.insert(id, slot);
								}
								rt::dbg!();

								use rt::process::Process;
								let proc = Process::new_by_name(
									driver,
									Process::default_handles(),
									None::<&[u8]>.into_iter(),
									rt::args::Env::new(),
								)
								.expect("yay");
							}
							_ => unreachable!(),
						}
					} else if let Some(slot) = conf_driver.remove(&id) {
						rt::dbg!("done configuring");
					} else {
						let buf = buffer.unwrap();
						let p = alloc::format!("{:?}", unsafe { buf.as_ref() });
						rt::dbg!(p);
						ctrl.transfer(slot, 3.try_into().unwrap(), buf, true);

						//unreachable!()
					}
				}
				Event::DeviceConfigured { slot, .. } => {
					for _ in 0..16 {
						let buf = dma::Dma::new_slice(8).unwrap();
						ctrl.transfer(slot, 3.try_into().unwrap(), buf, true);
					}
				}
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
					Object::List { slot } => {
						if let Some(s) = ctrl.next_slot(NonZeroU8::new(*slot)) {
							*slot = s.get();
							Job::get_info(&mut jobs, &mut ctrl, s, job_id);
							continue 'req;
						} else {
							*slot = 255;
							Response::Data(tbl.alloc(0).unwrap())
						}
					}
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
}

enum JobState {
	WaitDeviceInfo,
	WaitDeviceName { info: requests::Device },
}

impl Job {
	fn get_info(
		jobs: &mut BTreeMap<u64, Self>,
		ctrl: &mut xhci::Xhci,
		slot: NonZeroU8,
		job_id: JobId,
	) {
		let id = ctrl
			.send_request(
				slot,
				requests::Request::GetDescriptor {
					buffer: dma::Dma::new_slice(64).unwrap(),
					ty: requests::GetDescriptor::Device,
				},
			)
			.unwrap_or_else(|_| todo!());
		jobs.insert(
			id,
			Self {
				state: JobState::WaitDeviceInfo,
				job_id,
			},
		);
	}

	fn progress<'a>(
		mut self,
		jobs: &mut BTreeMap<u64, Self>,
		ctrl: &mut xhci::Xhci,
		slot: NonZeroU8,
		buf: dma::Dma<[u8]>,
		tbl: &'a StreamTable,
	) -> Option<(JobId, Response<'a>)> {
		let res = requests::DescriptorResult::decode(unsafe { buf.as_ref() });
		match &self.state {
			JobState::WaitDeviceInfo => {
				let info = res.into_device().unwrap();
				let id = ctrl
					.send_request(
						slot,
						requests::Request::GetDescriptor {
							buffer: buf,
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
				let s = res.into_string().unwrap();
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
}
