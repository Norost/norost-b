#![feature(path_try_exists)]

use std::ffi;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::str;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() {
	let table = 'tbl: loop {
		for f in fs::read_dir("").unwrap().map(Result::unwrap) {
			if &*f.file_name() == ffi::OsStr::new("virtio-net") {
				break 'tbl f;
			}
		}
		thread::sleep(Duration::from_millis(1));
	};
	let mut path = table.path();
	eprintln!("creating connection");
	//path.push("tcp&::ffff:10.0.2.2&6666");
	path.push("tcp&::ffff:172.106.11.86&6667"); // irc.libera.chat
	let mut f = File::create(path).unwrap();
	eprintln!("created connection");

	{
		let mut f = f.try_clone().unwrap();
		thread::spawn(move || {
			let mut buf = [0; 1024];
			let pre = b"\r\x1b[2K";
			buf[..pre.len()].copy_from_slice(pre);
			loop {
				eprint!("\r");
				let l = f.read(&mut buf[1..]).unwrap();
				io::stderr().write(&buf[..1 + l]).unwrap();
				thread::sleep(Duration::from_millis(50));
			}
		});
	}

	eprintln!("sending nick & user");
	f.write(b"NICK norostb\nUSER norostb * * :norostb\n")
		.unwrap();

	let mut buf = [0; 1024];
	let mut len = 0;
	loop {
		let prev_len = len;
		len += io::stdin().read(&mut buf[len..]).unwrap();
		for i in (prev_len..len).rev() {
			if buf[i] == 0x7f {
				// backspace
				buf.copy_within(i + 1.., i.saturating_sub(1));
				len = len.saturating_sub(2);
			}
		}
		if prev_len < len && buf[prev_len..len].contains(&b'\n') {
			eprintln!();
			f.write(&buf[..len]).unwrap();
			len = 0;
		}
		eprint!(
			"\r\x1b[2Kirc < {}",
			str::from_utf8(&buf[..len]).unwrap_or("<invalid utf-8>")
		);
		thread::sleep(Duration::from_millis(50));
	}
}
