//! # Mass storage device (MSD) / Bulk-Bulk-Bulk (BBB) driver

#![no_std]
#![feature(start)]

extern crate alloc;

use driver_utils::os::stream_table::{Request, Response, StreamTable};
use rt_default as _;

mod bbb;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main();
	0
}

fn main() {
	let stdout = rt::io::stdout().unwrap();
	let stdin = rt::io::stdin().unwrap();

	let mut bulk_out @ mut bulk_in @ mut intr_in = None;

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
			"--bulk-out" => decode_ep(&mut bulk_out, &mut args),
			"--bulk-in" => decode_ep(&mut bulk_in, &mut args),
			"--intr-in" => decode_ep(&mut intr_in, &mut args),
			"--intr-out" | "--isoch-out" | "--isoch-in" => {
				panic!("did not expect {}", a)
			}
			_ => panic!("invalid argument {:?}", a),
		}
	}

	let bulk_out = bulk_out.expect("bulk OUT endpoint not specified");
	let bulk_in = bulk_in.expect("bulk IN endpoint not specified");

	let mut dev = bbb::Device::new(bulk_out, bulk_in, &stdout, &stdin);

	// Send inquiry since it seems to be required for MSD devices to work properly
	dev.transfer_in(
		scsi::Inquiry {
			allocation_length: 0x24,
			evpd: 0,
			page_code: 0,
			control: 0,
		},
		0x24,
	)
	.unwrap();

	let data = dev
		.transfer_in(
			scsi::ReadCapacity10 {
				_reserved: 0,
				control: 0,
			},
			8,
		)
		.unwrap();
	let attr = scsi::ReadCapacity10Data::from(<[u8; 8]>::try_from(&*data).unwrap());
	assert!(attr.block_length.is_power_of_two());
	let block_length_p2 = attr.block_length.trailing_zeros();
	let block_length_mask = attr.block_length - 1;

	let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 16 }).unwrap();
	let tbl = StreamTable::new(&buf, 512.try_into().unwrap(), 512.try_into().unwrap());

	ipc_usb::send_public_object(|d| stdout.write(d)).unwrap();
	stdout.share(tbl.public()).unwrap();

	let mut obj = driver_utils::Arena::new();

	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((handle, job_id, req)) = tbl.dequeue() {
			let resp = match req {
				Request::Open { path } if handle == rt::Handle::MAX => {
					let mut p = alloc::vec![0; path.len()];
					path.copy_to(0, &mut p);
					if &p == b"data" {
						Response::Handle(obj.insert(0))
					} else {
						Response::Error(rt::Error::DoesNotExist)
					}
				}
				Request::Read { amount } if handle != rt::Handle::MAX => {
					if amount != attr.block_length {
						Response::Error(rt::Error::InvalidData)
					} else {
						let data = dev
							.transfer_in(
								scsi::Read10 {
									flags: 0,
									address: obj[handle],
									length: 1,
									group_number: 0,
									control: 0,
								},
								amount,
							)
							.unwrap();
						let b = tbl.alloc(amount as _).expect("out of buffers");
						b.copy_from(0, &data);
						obj[handle] += 1;
						Response::Data(b)
					}
				}
				Request::Write { data } if handle != rt::Handle::MAX => {
					if data.len() != attr.block_length as _ {
						Response::Error(rt::Error::InvalidData)
					} else {
						let mut b = alloc::vec![0; data.len()];
						data.copy_to(0, &mut b);
						dev.transfer_out(
							scsi::Write10 {
								flags: 0,
								address: obj[handle],
								length: 1,
								group_number: 0,
								control: 0,
							},
							&b,
						)
						.unwrap();
						obj[handle] += 1;
						Response::Amount(data.len() as _)
					}
				}
				Request::Seek { from } => match from {
					rt::io::SeekFrom::Start(n)
						if n & u64::from(block_length_mask) == 0
							&& n >> block_length_p2
								<= u64::from(attr.returned_logical_block_address) =>
					{
						obj[handle] = (n >> block_length_p2) as _;
						Response::Position(n)
					}
					_ => Response::Error(rt::Error::InvalidData),
				},
				Request::Close => {
					if handle != rt::Handle::MAX {
						obj.remove(handle).unwrap();
					}
					continue;
				}
				Request::GetMeta { property } => match property.get(&mut [0; 255]) {
					_ => Response::Error(rt::Error::InvalidData),
				},
				_ => Response::Error(rt::Error::InvalidOperation),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
	}
}
