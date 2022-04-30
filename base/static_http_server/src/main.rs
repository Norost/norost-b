use std::io::{Read, Write};
use std::net::TcpListener;

fn main() {
	eprintln!("yield because OS is kinda shit rn");
	std::thread::yield_now();

	if true {
		std::net::TcpStream::connect("10.0.2.2:6666")
			.unwrap()
			.write(b"hello!\n")
			.unwrap();
		println!("bye!");
	} else {
		eprintln!("creating listener");
		let listener = TcpListener::bind("0.0.0.0:80").unwrap();
		eprintln!("accepting incoming connections");
		for c in listener.incoming() {
			dbg!(c);
		}
	}
}
