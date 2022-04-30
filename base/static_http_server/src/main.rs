use std::{
	io::{Read, Write},
	net::TcpListener,
	time::Duration,
};

fn main() {
	eprintln!("yield because OS is kinda shit rn");
	std::thread::yield_now();

	eprintln!("creating listener");
	let listener = TcpListener::bind("0.0.0.0:80").unwrap();
	eprintln!("accepting incoming connections");

	let mut buf = [0; 1 << 11];
	let mut buf2 = [0; 1 << 14];

	for mut c in listener.incoming().map(Result::unwrap) {
		loop {
			let mut headers = [""; 128];
			c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
			c.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
			let (data, keep_alive) = match c.read(&mut buf) {
				Ok(0) => continue,
				Ok(n) => match mhttp::RequestParser::parse(&buf[..n], &mut headers) {
					Ok((req, _)) => handle_client(req, &mut buf2),
					Err(_) => (bad_request(&mut buf2), false),
				},
				Err(_) => (bad_request(&mut buf2), false),
			};
			let _ = c.write(data);
			let _ = c.write(b"hey");
			if !keep_alive {
				break;
			}
			break; // TODO
		}
	}
}

fn handle_client<'a>(request: mhttp::RequestParser, buf: &'a mut [u8]) -> (&'a [u8], bool) {
	let keep_alive = request.header("connection") == Some("keep-alive");
	let r = match request.header("host") {
		None => bad_request(buf),
		Some(_) => {
			mhttp::ResponseBuilder::new(buf, mhttp::Status::Ok)
				.unwrap()
				.add_header("content-length", "3")
				.unwrap()
				.finish()
				.0
		}
	};
	(r, keep_alive)
}

fn bad_request(buf: &mut [u8]) -> &[u8] {
	mhttp::ResponseBuilder::new(buf, mhttp::Status::BadRequest)
		.unwrap()
		.finish()
		.0
}
