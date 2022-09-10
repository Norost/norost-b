//! # USB driver

#![no_std]
#![feature(start)]
#![feature(ptr_as_uninit)]
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
mod xhci;

use {
	alloc::{boxed::Box, collections::BTreeMap, vec::Vec},
	core::{future::Future, num::NonZeroU8, pin::Pin, str, task::Context, time::Duration},
	dma::Dma,
	driver_utils::{
		os::stream_table::{JobId, Request, Response, StreamTable},
		task::waker,
	},
	io_queue_rt::{Pow2Size, Queue},
	rt::{Error, Handle},
	rt_default as _,
	usb_request::descriptor::{Configuration, Descriptor, Device, Endpoint, Interface},
};

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
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

	let mut transfers = BTreeMap::default();
	let mut wait_finish_config = BTreeMap::default();

	enum Transfer<'a> {
		Job(Job),
		GetDevice,
		GetConfiguration(GetConfiguration),
		SetConfiguration(Box<SetConfiguration<'a>>),
	}
	struct GetConfiguration {
		device: Device,
	}
	struct SetConfiguration<'a> {
		driver: &'a config::Driver,
		endpoints: Vec<Endpoint>,
		interface: Interface,
		device: Device,
		config: Configuration,
	}
	struct EvaluateContext<'a> {
		driver: &'a config::Driver,
		endpoints: Vec<Endpoint>,
		interface: Interface,
		device: Device,
		config: Configuration,
	}

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
						let buffer = Dma::new_slice(1024).unwrap_or_else(|_| todo!());
						let e = ctrl
							.send_request(
								slot,
								usb_request::Request::GetDescriptor {
									ty: usb_request::descriptor::GetDescriptor::Device,
								},
								buffer,
							)
							.unwrap_or_else(|_| todo!());
						trace!("id {:x}", e);
						transfers.insert(e, Transfer::GetDevice);
					}
					Event::Transfer { slot, endpoint, id, buffer, code } => {
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
						if let Some(trf) = transfers.remove(&id) {
							match trf {
								Transfer::Job(mut j) => {
									trace!("Job");
									match j.progress(&mut ctrl, slot, buffer.unwrap(), &tbl) {
										JobResult::Done { job_id, response } => {
											trace!("finish job");
											tbl.enqueue(job_id, response);
											tbl.flush();
										}
										JobResult::Next { id, job } => {
											trace!("continue job");
											transfers.insert(id, Transfer::Job(job));
										}
									}
								}
								Transfer::GetDevice => {
									trace!("GetDevice");
									let buffer = buffer.unwrap();
									let mut it =
										usb_request::descriptor::decode(unsafe { buffer.as_ref() });
									let device = it.next().unwrap().unwrap().into_device().unwrap();
									let base = (device.class, device.subclass, device.protocol);
									info!(
										"slot {}: device {:02x}/{:02x}/{:02x}",
										slot, device.class, device.subclass, device.protocol
									);
									let id = ctrl
										.send_request(
											slot,
											usb_request::Request::GetDescriptor {
												ty: usb_request::descriptor::GetDescriptor::Configuration {
													index: 0,
												},
											},
											buffer,
										)
										.unwrap_or_else(|_| todo!());
									transfers.insert(
										id,
										Transfer::GetConfiguration(GetConfiguration { device }),
									);
								}
								Transfer::GetConfiguration(j) => {
									trace!("GetConfiguration");
									let buffer = buffer.unwrap();
									let mut it =
										usb_request::descriptor::decode(unsafe { buffer.as_ref() });
									let config =
										it.next().unwrap().unwrap().into_configuration().unwrap();
									let mut n = usize::from(config.num_interfaces);
									let mut driver = None;
									let mut endpoints = Vec::new();
									let mut last_intf = None;
									let base =
										(j.device.class, j.device.subclass, j.device.protocol);
									while n > 0 {
										match it.next().unwrap().unwrap() {
											Descriptor::Interface(i) => {
												last_intf = Some(i.index);
												info!(
													"slot {}: interface {:02x}/{:02x}/{:02x}",
													slot, i.class, i.subclass, i.protocol
												);
												let intf = (i.class, i.subclass, i.protocol);
												if driver.is_none() {
													n += usize::from(i.num_endpoints);
													conf.get_driver(base, intf)
														.map(|d| driver = Some((d, i)));
												} else {
													break;
												}
												n -= 1;
											}
											Descriptor::Endpoint(e) => {
												if driver.is_some() {
													endpoints.push(e)
												}
												n -= 1;
											}
											Descriptor::Unknown { ty, .. } => {
												warn!("Unknown descriptor type {}", ty);
											}
											Descriptor::Device(_) => {
												todo!("unexpected")
											}
											Descriptor::Configuration(_) => {
												todo!("unexpected")
											}
											Descriptor::String(_) => {
												todo!("unexpected")
											}
											Descriptor::Hid(_) => {}
										}
									}

									let Some((driver, interface)) = driver else {
										info!("no driver found");
										continue;
									};

									let id = ctrl
										.send_request(
											slot,
											usb_request::Request::SetConfiguration {
												value: config.index_configuration,
											},
											Dma::new_slice(0).unwrap(),
										)
										.unwrap_or_else(|_| todo!());
									transfers.insert(
										id,
										Transfer::SetConfiguration(
											SetConfiguration {
												device: j.device,
												driver,
												interface,
												endpoints,
												config,
											}
											.into(),
										),
									);
								}
								Transfer::SetConfiguration(c) => {
									trace!("SetConfiguration");
									let id = ctrl.configure_device(
										slot,
										xhci::DeviceConfig {
											config: &c.config,
											interface: &c.interface,
											endpoints: &c.endpoints,
										},
									);
									wait_finish_config.insert(
										id,
										EvaluateContext {
											driver: c.driver,
											endpoints: c.endpoints,
											interface: c.interface,
											device: c.device,
											config: c.config,
										}
										.into(),
									);
								}
							}
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
						let c: EvaluateContext = wait_finish_config.remove(&id).unwrap();
						let base = (c.device.class, c.device.subclass, c.device.protocol);
						let intf = (
							c.interface.class,
							c.interface.subclass,
							c.interface.protocol,
						);
						drivers
							.load_driver(slot, c.driver, base, intf, &c.endpoints)
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
					let mut buf = Dma::new_slice(size.try_into().unwrap()).unwrap();
					ctrl.transfer(slot, ep.try_into().unwrap(), buf, true)
				}
				Event::DataOut { endpoint, data } => {
					assert!(endpoint > 0);
					let ep = endpoint << 1;
					assert!(ep < 32);
					ctrl.transfer(slot, ep.try_into().unwrap(), data, false)
				}
				Event::GetDescriptor { recipient, ty, index, len } => {
					use usb_request::RawRequest as R;
					let buf = Dma::new_slice(len.into()).unwrap();
					let recipient = match recipient {
						driver::Recipient::Device => R::RECIPIENT_DEVICE,
						driver::Recipient::Interface => R::RECIPIENT_INTERFACE,
					};
					let req = R {
						request_type: R::DIR_IN | R::TYPE_STANDARD | recipient,
						request: R::GET_DESCRIPTOR,
						value: u16::from(ty) << 8 | u16::from(index),
						index: 0,
					};
					ctrl.send_request(slot, req, buf).map_err(|_| todo!())
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
								let (id, job) = Job::get_info(&mut ctrl, s, job_id);
								transfers.insert(id, Transfer::Job(job));
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
	fn get_info(ctrl: &mut xhci::Xhci, slot: NonZeroU8, job_id: JobId) -> (u64, Self) {
		let buffer = Dma::new_slice(64).unwrap();
		let id = ctrl
			.send_request(
				slot,
				usb_request::Request::GetDescriptor {
					ty: usb_request::descriptor::GetDescriptor::Device,
				},
				buffer,
			)
			.unwrap_or_else(|_| todo!());
		(id, Self { state: JobState::WaitDeviceInfo, job_id })
	}

	fn progress<'a>(
		mut self,
		ctrl: &mut xhci::Xhci,
		slot: NonZeroU8,
		buf: Dma<[u8]>,
		tbl: &'a StreamTable,
	) -> JobResult<'a> {
		let res = usb_request::descriptor::decode(unsafe { buf.as_ref() })
			.next()
			.unwrap()
			.unwrap();
		match &self.state {
			JobState::WaitDeviceInfo => {
				let info = res.into_device().unwrap();
				if info.index_product != 0 {
					let id = ctrl
						.send_request(
							slot,
							usb_request::Request::GetDescriptor {
								ty: usb_request::descriptor::GetDescriptor::String {
									index: info.index_product,
								},
							},
							Dma::new_slice(0).unwrap(),
						)
						.unwrap_or_else(|_| todo!());
					self.state = JobState::WaitDeviceName;
					JobResult::Next { id, job: self }
				} else {
					let name = tbl.alloc(3).expect("out of buffers");
					name.copy_from(0, b"N/A");
					JobResult::Done { job_id: self.job_id, response: Response::Data(name) }
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
				JobResult::Done { job_id: self.job_id, response: Response::Data(name) }
			}
		}
	}
}

enum JobResult<'a> {
	Next { id: u64, job: Job },
	Done { job_id: JobId, response: Response<'a, 'static> },
}
