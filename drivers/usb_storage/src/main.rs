//! # Mass storage device (MSD) / Bulk-Bulk-Bulk (BBB) driver

#![no_std]
#![feature(start)]

extern crate alloc;

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

	let data = bbb::transfer_in(
		&stdout,
		&stdin,
		scsi::Inquiry {
			allocation_length: 0x24,
			evpd: 0,
			page_code: 0,
			control: 0,
		},
		0x24,
	)
	.unwrap();
	rt::dbg!(alloc::string::String::from_utf8_lossy(&data));

	let data = bbb::transfer_in(
		&stdout,
		&stdin,
		scsi::ReadCapacity10 {
			_reserved: 0,
			control: 0,
		},
		8,
	)
	.unwrap();
	rt::dbg!(scsi::ReadCapacity10Data::from(
		<[u8; 8]>::try_from(&*data).unwrap()
	));

	bbb::transfer_out(
		&stdout,
		&stdin,
		scsi::Write10 {
			flags: 0,
			address: 0,
			length: 1,
			group_number: 0,
			control: 0,
		},
		&[b'c'; 512],
	)
	.unwrap();
}
