#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use ipc_usb::Recv;
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let cfg = load_keymap();
	rt::dbg!(cfg);

	let stdout = rt::io::stdout().unwrap();
	let stdin = rt::io::stdin().unwrap();

	let (public_in, public_out) = rt::Object::new(rt::NewObject::MessagePipe).unwrap();

	ipc_usb::send_intr_in_enqueue_num(1, 16, |d| stdout.write(d)).unwrap();
	ipc_usb::send_public_object(|d| stdout.write(d)).unwrap();
	stdout.share(&public_out).unwrap();

	loop {
		let mut buf = [0; 32];
		let len = stdin.read(&mut buf).unwrap();
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::IntrIn { ep, data } => {
				for d in data {
					rt::eprint!("{:02x} ", d);
				}
				rt::eprintln!()
			}
		}
	}
}

fn load_keymap() -> scancodes::config::Config {
	let f = rt::io::file_root()
		.unwrap()
		.open(b"drivers/keyboard.scf")
		.unwrap();
	let len = f
		.seek(rt::io::SeekFrom::End(0))
		.unwrap()
		.try_into()
		.unwrap();
	f.seek(rt::io::SeekFrom::Start(0)).unwrap();
	let mut buf = Vec::with_capacity(len);
	let mut offt = 0;
	while offt < len {
		offt += f
			.read_uninit(&mut buf.spare_capacity_mut()[offt..])
			.unwrap()
			.0
			.len();
	}
	unsafe { buf.set_len(len) };
	scancodes::config::parse(&buf).unwrap()
}
