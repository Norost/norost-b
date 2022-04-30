use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
	eprintln!("yield because OS is kinda shit rn");
	std::thread::yield_now();

	eprintln!("creating listener");
	let listener = TcpListener::bind("0.0.0.0:80").unwrap();
	eprintln!("accepting incoming connections");
	for mut c in listener.incoming().map(Result::unwrap) {
		eprintln!("accepted!");
		c.write(b"hey\n").unwrap();
		eprintln!("written");
	}
}
