#![feature(path_try_exists)]

fn main() {
	let table = 'tbl: loop {
		for f in std::fs::read_dir("").unwrap().map(Result::unwrap) {
			if &*f.file_name() == std::ffi::OsStr::new("virtio-net") {
				break 'tbl f;
			}
		}
		std::thread::sleep(std::time::Duration::from_secs(1));
	};
	let mut path = table.path();
	path.push("tcp&::ffff:10.0.2.2&6666");
	let mut f = std::fs::File::create(path).unwrap();

	let mut buf = [0; 1024];
	let mut len = 0;
	loop {
		use std::io::{Read, Write};
		len += std::io::stdin().read(&mut buf[len..]).unwrap();
		if buf[..len].contains(&b'\n') {
			eprintln!();
			f.write(&buf[..len]).unwrap();
			len = 0;
		}
		eprint!("\rirc < ");
		for _ in 0..len {
			eprint!(" ");
		}
		eprint!(
			"\rirc < {}",
			std::str::from_utf8(&buf[..len]).unwrap_or("<invalid utf-8>")
		);
		std::thread::sleep(std::time::Duration::from_millis(10));
	}
}
