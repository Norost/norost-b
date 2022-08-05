//! # USB driver

#![no_std]
#![feature(start)]

extern crate alloc;

mod dma;
mod xhci;

use alloc::vec::Vec;
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
	let mut init_dev_alloc: Vec<(_, xhci::device::AllocSlot)> = Vec::new();
	let mut init_dev_addr: Vec<(_, xhci::device::SetAddress)> = Vec::new();

	let mut devs = Vec::new();

	loop {
		if let Some(e) = ctrl.dequeue_event() {
			match e {
				Event::PortStatusChange { port } => {
					rt::dbg!();
					let wait = ctrl.init_device(port).unwrap();
					rt::dbg!();
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
						devs.push(d);
						devs.last_mut().unwrap().test(&mut ctrl);
					} else {
						todo!()
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
		rt::thread::sleep(core::time::Duration::from_millis(100));
	}
}
