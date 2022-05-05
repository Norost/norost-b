#![feature(norostb)]

use std::{io, os::norostb::prelude::*, process::Command};

fn main() -> Result<(), io::Error> {
	let mut args = std::env::args_os().skip(1);
	let dir = args.next().unwrap();
	assert_eq!(args.next().unwrap().as_bytes(), b"--");
	let program = args.next().unwrap();
	dbg!(&dir, &program);
	Command::new(program)
		.current_dir(dir)
		.spawn()
		.map(|_| ())
		.unwrap();

	loop {
		// TODO ditto
		std::thread::sleep(std::time::Duration::MAX);
	}
}
