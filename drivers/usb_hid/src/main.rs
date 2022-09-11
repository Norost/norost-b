#![no_std]
#![feature(start)]

extern crate alloc;

mod report;
mod translate;

use {input::Input, ipc_usb::Recv, rt_default as _};

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let stdout = rt::io::stdout().unwrap();
	let stdin = rt::io::stdin().unwrap();

	let mut intr_in = None;

	let mut args = rt::args::args().skip(1);
	while let Some(a) = args.next() {
		let a = core::str::from_utf8(a).unwrap();
		let decode_ep = |v: &mut Option<_>, args: &mut dyn Iterator<Item = &[u8]>| {
			let n = args.next().expect("expected argument");
			let ep = ipc_usb::Endpoint::try_from(n).expect("invalid endpoint");
			let prev = v.replace(ep);
			assert!(prev.is_none(), "{} already specified", a);
		};
		match a {
			"--class" => {
				// Just ignore for now.
				args.next().unwrap();
			}
			"--intr-in" => decode_ep(&mut intr_in, &mut args),
			"--bulk-out" | "--bulk-in" | "--intr-out" | "--isoch-out" | "--isoch-in" => {
				panic!("did not expect {}", a)
			}
			"--configuration" | "--interface" => {}
			_ => panic!("invalid argument {:?}", a),
		}
	}

	// Parse report descriptor
	let report = {
		ipc_usb::send_get_descriptor(0, 2, 0, 256, |d| stdout.write(d)).unwrap();
		let mut buf = [0; 2 + 256];
		let len = stdin.read(&mut buf).unwrap();
		let mut report_len = None;
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::DataIn { ep: 0, data } => {
				for d in usb_request::descriptor::decode(data).map(|r| r.unwrap()) {
					if let usb_request::descriptor::Descriptor::Hid(d) = d {
						report_len = Some(d.len);
						rt::dbg!(d);
						break;
					}
					rt::dbg!(d);
				}
			}
			_ => todo!(),
		}

		let len = report_len.unwrap();
		ipc_usb::send_get_descriptor(1, 0x22, 0, len, |d| stdout.write(d)).unwrap();
		let mut buf = [0; 512];
		let len = stdin.read(&mut buf).unwrap();
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::DataIn { ep: 0, data } => report::parse(data),
			_ => todo!(),
		}
	};

	let (public_in, public_out) = rt::Object::new(rt::NewObject::MessagePipe).unwrap();

	let intr_in = intr_in.unwrap();
	let enqueue_read =
		|| ipc_usb::send_data_in(intr_in, report.size(), |d| stdout.write(d)).unwrap();
	for _ in 0..16 {
		enqueue_read();
	}
	ipc_usb::send_public_object(|d| stdout.write(d)).unwrap();
	stdout.share(&public_out).unwrap();

	let mut input_buf = alloc::vec::Vec::new();
	loop {
		let mut buf = [0; 32];
		let len = stdin.read(&mut buf).unwrap();
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::DataIn { ep: _, data } => {
				enqueue_read();
				let mut offt = 0;
				input_buf.clear();
				for (usages, f) in &report.fields {
					if usages.is_empty() {
						// padding
						offt += f.report_size * f.report_count;
						continue;
					}
					for i in 0..f.report_count {
						let v = f.extract_u32(data, offt).unwrap();
						let u = usages.get(i).unwrap();
						let k = translate::hid_to_keycode(u, f.flags);
						offt += f.report_size;

						if let Some(k) = k {
							assert_eq!(f.logical_min, 0, "todo");
							let lvl = v as u64 * (1 << 31) / (f.logical_max as u64 + 1);
							let evt = Input::new(k, lvl as _);
							input_buf.extend_from_slice(&u64::from(evt).to_le_bytes());
						}
					}
				}
				public_in.write(&input_buf).unwrap();
			}
			Recv::Error { id, code, message } => {
				panic!("{} (message {}, code {})", message, id, code)
			}
			_ => todo!(),
		}
	}
}
