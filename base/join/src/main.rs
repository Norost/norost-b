#![no_std]
#![feature(start)]

use rt_default as _;

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let inp = rt::io::stdin().expect("in undefined");
	let out = rt::io::stdout().expect("out undefined");

	let mut buf = [0; 1 << 15];
	loop {
		let l = inp.read(&mut buf).expect("failed to read");
		out.write_all(&mut buf[..l]).expect("failed to write");
	}
}
