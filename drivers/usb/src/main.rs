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

	let mut devs = BTreeMap::new();

	let mut pending_get_descr = BTreeMap::default();

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
						{
							let buf = dma::Dma::new_slice(64).unwrap();
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
							pending_get_descr.insert(e, buf);

							let buf = dma::Dma::new_slice(64).unwrap();
							let e = d
								.send_request(
									&mut ctrl,
									0,
									requests::Request::GetDescriptor {
										buffer: &buf,
										ty: requests::GetDescriptor::String { index: 2 },
									},
								)
								.unwrap_or_else(|_| todo!());
							pending_get_descr.insert(e, buf);

							let buf = dma::Dma::new_slice(64).unwrap();
							let e = d
								.send_request(
									&mut ctrl,
									0,
									requests::Request::GetDescriptor {
										buffer: &buf,
										ty: requests::GetDescriptor::Configuration { index: 0 },
									},
								)
								.unwrap_or_else(|_| todo!());
							pending_get_descr.insert(e, buf);
						}
					} else {
						todo!()
					}
				}
				Event::Transfer { id, slot } => {
					let buf = pending_get_descr.remove(&id).expect("invalid id");
					let buf = unsafe { buf.as_ref() };
					match requests::DescriptorResult::decode(buf).unwrap_or_else(|_| todo!()) {
						requests::DescriptorResult::Device(d) => {
							rt::dbg!(d);
						}
						requests::DescriptorResult::Configuration(c) => {
							rt::dbg!(c);
						}
						requests::DescriptorResult::String(s) => {
							let s = char::decode_utf16(s)
								.map(Result::unwrap)
								.collect::<alloc::string::String>();
							rt::dbg!(s);
						}
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
		rt::thread::sleep(core::time::Duration::from_millis(10));
	}
}
