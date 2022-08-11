#![no_std]
#![feature(start)]

use ipc_usb::Recv;
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let stdout = rt::io::stdout().unwrap();
	let stdin = rt::io::stdin().unwrap();

	ipc_usb::send_intr_in_enqueue_num(1, 16, |d| stdout.write(d)).unwrap();

	loop {
		let mut buf = [0; 32];
		let len = stdin.read(&mut buf).unwrap();
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::IntrIn { ep, data } => {
				rt::dbg!(data);
			}
		}
	}
}
