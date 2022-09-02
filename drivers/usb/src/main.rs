//! # USB driver

#![no_std]
#![feature(start)]
#![feature(inline_const, const_option)]
#![feature(let_else)]
#![feature(array_chunks)]
#![feature(alloc_layout_extra)]
#![feature(nonnull_slice_from_raw_parts, ptr_metadata, slice_ptr_get)]
#![feature(result_option_inspect)]
#![feature(closure_lifetime_binder)]
#![feature(iterator_try_collect)]
#![feature(optimize_attribute)]

extern crate alloc;

#[doc(hidden)]
#[inline(never)]
#[optimize(size)]
#[cold]
fn _message(pre: &str, args: core::fmt::Arguments<'_>) {
	rt::eprintln!("[{}] [{}] {}", rt::time::Monotonic::now(), pre, args);
}

#[cfg(feature = "trace")]
macro_rules! trace {
	($($arg:tt)*) => {{
		$crate::_message(file!(), format_args!($($arg)*));
	}};
}
#[cfg(not(feature = "trace"))]
macro_rules! trace {
	($($arg:tt)*) => {{
		let _ = || ($($arg)*);
	}};
}

macro_rules! info {
	($($arg:tt)*) => {{
		$crate::_message("INFO", format_args!($($arg)*));
	}};
}
macro_rules! warn {
	($($arg:tt)*) => {{
		$crate::_message("WARN", format_args!($($arg)*));
	}};
}

mod config;
mod dma;
mod driver;
mod loader;
mod requests;
mod xhci;

use alloc::{collections::BTreeMap, vec::Vec};
use core::{future::Future, num::NonZeroU8, pin::Pin, str, task::Context, time::Duration};
use driver_utils::{
	os::stream_table::{JobId, Request, Response, StreamTable},
	task::waker,
};
use io_queue_rt::{Pow2Size, Queue};
use rt::{Error, Handle};
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	rt::thread::sleep(Duration::from_millis(200));
	main()
}

fn main() -> ! {
	let file_root = rt::io::file_root().expect("no file root");
	let conf = config::parse(&file_root.open(b"drivers/usb.scf").unwrap());

	let dev = rt::args::handles()
		.find(|(name, _)| name == b"pci")
		.expect("no 'pci' object")
		.1;

	let queue = Queue::new(Pow2Size::P5, Pow2Size::P7).unwrap();
	let mut ctrl = xhci::Xhci::new(&dev).unwrap();
	let mut drivers = driver::Drivers::new(&queue);

	let mut jobs = BTreeMap::<u64, Job>::default();
	let mut load_driver = BTreeMap::<u64, LoadDriver>::default();

	let mut conf_driver = BTreeMap::default();
	let mut wait_finish_config = BTreeMap::default();

	let (tbl_buf, _) = driver_utils::dma::alloc_dma_object((1 << 20).try_into().unwrap()).unwrap();
	let tbl = StreamTable::new(&tbl_buf, 512.try_into().unwrap(), (1 << 12) - 1);
	file_root
		.create(b"usb")
		.unwrap()
		.share(tbl.public())
		.unwrap();
	let mut objects = driver_utils::Arena::new();

	let mut poll_ctrl = queue.submit_read(ctrl.notifier().as_raw(), ()).unwrap();
	let mut poll_tbl = queue.submit_read(tbl.notifier().as_raw(), ()).unwrap();

	loop {
		let w = waker::dummy();
		let mut cx = Context::from_waker(&w);
		if Pin::new(&mut poll_ctrl).poll(&mut cx).is_ready() {
			trace!("controller events");
			poll_ctrl = queue.submit_read(ctrl.notifier().as_raw(), ()).unwrap();
			while let Some(e) = ctrl.poll() {
				use self::xhci::Event;
				match e {
					Event::NewDevice { slot } => {
						trace!("new device, slot {}", slot);
						let buffer = dma::Dma::new_slice(1024).unwrap_or_else(|_| todo!());
						let e = ctrl
							.send_request(
								slot,
								requests::Request::GetDescriptor {
									buffer,
									ty: requests::GetDescriptor::Device,
								},
							)
							.unwrap_or_else(|_| todo!());
						trace!("id {:x}", e);
						load_driver.insert(e, LoadDriver { base: None });
					}
					Event::Transfer {
						slot,
						endpoint,
						id,
						buffer,
						code,
					} => {
						trace!(
							"transfer, slot {} ep {} id {:x}, {:?}",
							slot,
							endpoint,
							id,
							code
						);
						use ::xhci::ring::trb::event::CompletionCode;
						match code {
							Ok(CompletionCode::Success) | Ok(CompletionCode::ShortPacket) => {}
							e => todo!("{:?}", e),
						}
						if let Some(j) = jobs.remove(&id) {
							trace!("progress job");
							if let Some((job_id, resp)) =
								j.progress(&mut jobs, &mut ctrl, slot, buffer.unwrap(), &tbl)
							{
								trace!("finish job");
								tbl.enqueue(job_id, resp);
								tbl.flush();
							}
						} else if let Some(mut j) = load_driver.remove(&id) {
							trace!("load driver");
							let buffer = buffer.unwrap();
							let mut it = requests::decode(unsafe { buffer.as_ref() });
							match it.next().unwrap() {
								requests::DescriptorResult::Device(info) => {
									j.base = Some((info.class, info.subclass, info.protocol));
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
								}
								requests::DescriptorResult::Configuration(config) => {
									let base = j.base.unwrap();
									let mut n = usize::from(config.num_interfaces);
									let mut driver = None;
									let mut endpoints = Vec::new();
									while n > 0 {
										match it.next().unwrap() {
											requests::DescriptorResult::Interface(i) => {
												let intf = (i.class, i.subclass, i.protocol);
												if driver.is_none() {
													n += usize::from(i.num_endpoints);
													conf.get_driver(base, intf)
														.map(|d| driver = Some((d, i, intf)));
												} else {
													break;
												}
											}
											requests::DescriptorResult::Endpoint(e) => {
												if driver.is_some() {
													endpoints.push(e)
												}
											}
											requests::DescriptorResult::Unknown { .. } => continue,
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

									let Some((driver, interface, intf)) = driver else {
										trace!("no driver found");
										continue;
									};

									let id = ctrl
										.send_request(
											slot,
											requests::Request::SetConfiguration {
												value: config.index_configuration,
											},
										)
										.unwrap_or_else(|_| todo!());
									conf_driver.insert(
										id,
										(config, driver, interface, intf, endpoints, base),
									);
								}
								requests::DescriptorResult::String(_) => todo!(),
								requests::DescriptorResult::Endpoint(_) => todo!(),
								requests::DescriptorResult::Unknown { .. } => todo!(),
								requests::DescriptorResult::Interface(_) => todo!(),
								requests::DescriptorResult::Truncated { .. } => todo!(),
								requests::DescriptorResult::Invalid => todo!(),
							}
						} else if let Some((config, driver, interface, intf, endpoints, base)) =
							conf_driver.remove(&id)
						{
							trace!("SetConfigured");
							let id = ctrl.configure_device(
								slot,
								xhci::DeviceConfig {
									config: &config,
									interface: &interface,
									endpoints: &endpoints,
								},
							);
							wait_finish_config.insert(id, (driver, intf, endpoints, base));
						} else {
							trace!("driver transfer");
							let buf = buffer.unwrap();
							assert!(endpoint & 1 == 1);
							drivers
								.send(
									slot,
									driver::Message::DataIn {
										endpoint: endpoint >> 1,
										data: unsafe { buf.as_ref() },
									},
								)
								.unwrap();
						}
					}
					Event::DeviceConfigured { slot, id, code } => {
						assert_eq!(code, Ok(::xhci::ring::trb::event::CompletionCode::Success));
						trace!("configured device slot {}, {:?}", slot, code);
						let (driver, intf, endpoints, base) =
							wait_finish_config.remove(&id).unwrap();
						drivers
							.load_driver(slot, driver, base, intf, &endpoints)
							.unwrap();
						code.unwrap();
					}
				}
			}
		}

		while let Some((slot, msg_id, evt)) = drivers.dequeue() {
			use driver::Event;
			let res = match evt {
				Event::DataIn { endpoint, size } => {
					assert!(endpoint > 0);
					let ep = endpoint << 1 | 1;
					assert!(ep < 32);
					let buf = dma::Dma::new_slice(size.try_into().unwrap()).unwrap();
					ctrl.transfer(slot, ep.try_into().unwrap(), buf, true)
				}
				Event::DataOut { endpoint, data } => {
					assert!(endpoint > 0);
					let ep = endpoint << 1;
					assert!(ep < 32);
					ctrl.transfer(slot, ep.try_into().unwrap(), data, false)
				}
			};
			match res {
				Ok(_id) => {}
				Err(e) => drivers
					.send(
						slot,
						match e {
							xhci::TransferError::InvalidEndpoint { endpoint } => {
								warn!(
									"driver tried to access invalid endpoint {} (slot {})",
									endpoint, slot
								);
								driver::Message::Error {
									id: msg_id,
									code: 1,
									message: "invalid endpoint",
								}
							}
						},
					)
					.unwrap_or_else(|e| todo!("{:?}", e)),
			}
		}

		#[derive(Debug)]
		enum Object {
			Root { i: u8 },
			ListDevices { slot: u8 },
			ListHandlers { index: usize },
		}

		if Pin::new(&mut poll_tbl).poll(&mut cx).is_ready() {
			poll_tbl = queue.submit_read(tbl.notifier().as_raw(), ()).unwrap();
			'req: while let Some((handle, job_id, req)) = tbl.dequeue() {
				let mut buf = [0; 64];
				let resp = match req {
					Request::Open { path } => match (handle, &*path.copy_into(&mut buf).0) {
						(Handle::MAX, b"") => {
							Response::Handle(objects.insert(Object::Root { i: 0 }))
						}
						(Handle::MAX, b"devices") | (Handle::MAX, b"devices/") => {
							Response::Handle(objects.insert(Object::ListDevices { slot: 0 }))
						}
						(Handle::MAX, b"handlers") | (Handle::MAX, b"handlers/") => {
							Response::Handle(objects.insert(Object::ListHandlers { index: 0 }))
						}
						(Handle::MAX, p) if p.starts_with(b"handlers/") => {
							let p = &p["handlers/".len()..];
							if let Ok(Some(h)) = str::from_utf8(p).map(|p| drivers.handler(p)) {
								Response::Object(h)
							} else {
								Response::Error(Error::DoesNotExist)
							}
						}
						_ => Response::Error(Error::DoesNotExist),
					},
					Request::Read { amount } => match &mut objects[handle] {
						Object::Root { i } => {
							let s: &[u8] = match i {
								0 => b"devices",
								1 => b"handlers",
								_ => {
									*i -= 1;
									b""
								}
							};
							*i += 1;
							let b = tbl.alloc(s.len().min(amount as _)).expect("out of buffers");
							b.copy_from(0, &s[..b.len()]);
							Response::Data(b)
						}
						Object::ListDevices { slot } => {
							if let Some(s) = ctrl.next_slot(NonZeroU8::new(*slot)) {
								*slot = s.get();
								Job::get_info(&mut jobs, &mut ctrl, s, job_id);
								//Job::get_string(&mut jobs, &mut ctrl, s, job_id);
								continue 'req;
							} else {
								*slot = 255;
								Response::Data(tbl.alloc(0).unwrap())
							}
						}
						Object::ListHandlers { index } => {
							if let Some((k, _)) = drivers.handler_at(*index) {
								*index += 1;
								let buf = tbl.alloc(k.len()).expect("out of buffers");
								buf.copy_from(0, k.as_ref());
								Response::Data(buf)
							} else {
								*index = usize::MAX;
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
		}

		queue.poll();
		queue.wait(Duration::MAX);
		queue.process();
	}
}

struct Job {
	state: JobState,
	job_id: JobId,
}

enum JobState {
	WaitDeviceInfo,
	WaitDeviceName,
}

impl Job {
	fn get_info(
		jobs: &mut BTreeMap<u64, Self>,
		ctrl: &mut xhci::Xhci,
		slot: NonZeroU8,
		job_id: JobId,
	) {
		let buffer = dma::Dma::new_slice(64).unwrap();
		let id = ctrl
			.send_request(
				slot,
				requests::Request::GetDescriptor {
					buffer,
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
	) -> Option<(JobId, Response<'a, 'static>)> {
		let res = requests::DescriptorResult::decode(unsafe { buf.as_ref() });
		match &self.state {
			JobState::WaitDeviceInfo => {
				let info = res.into_device().unwrap();
				//if info.index_product != 0 {
				if info.index_manufacturer != 0 {
					let id = ctrl
						.send_request(
							slot,
							requests::Request::GetDescriptor {
								buffer: buf,
								ty: requests::GetDescriptor::String {
									//index: info.index_product,
									index: info.index_manufacturer,
								},
							},
						)
						.unwrap_or_else(|_| todo!());
					self.state = JobState::WaitDeviceName;
					jobs.insert(id, self);
					None
				} else {
					let name = tbl.alloc(3).expect("out of buffers");
					name.copy_from(0, b"N/A");
					Some((self.job_id, Response::Data(name)))
				}
			}
			JobState::WaitDeviceName => {
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
