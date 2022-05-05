use std::{
	borrow::Cow,
	io::{ErrorKind, Read, Write},
	net::TcpListener,
	time::Duration,
};

const BAD_REQUEST: &[u8] = b"<!DOCTYPE html><h1>400 Bad Request/h1>";
const NOT_FOUND: &[u8] = b"<!DOCTYPE html><h1>404 Not Found</h1>";
const INTERNAL_SERVER_ERROR: &[u8] = b"<!DOCTYPE html><h1>500 Internal Server Error</h1>";

fn main() {
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
			let (header, body, keep_alive) = match c.read(&mut buf) {
				Ok(0) => continue,
				Ok(n) => match mhttp::RequestParser::parse(&buf[..n], &mut headers) {
					Ok((req, _)) => handle_client(req, &mut buf2),
					Err(_) => bad_request(&mut buf2, false),
				},
				Err(_) => bad_request(&mut buf2, false),
			};
			let _ = c.write(header);
			let _ = c.write(&body);
			if !keep_alive {
				break;
			}
			break; // TODO
		}
	}
}

fn handle_client<'a>(
	request: mhttp::RequestParser,
	buf: &'a mut [u8],
) -> (&'a [u8], Cow<'static, [u8]>, bool) {
	let keep_alive = request.header("connection") == Some("keep-alive");
	match request.header("host") {
		None => bad_request(buf, keep_alive),
		Some(_) if !request.path.starts_with("/") => bad_request(buf, keep_alive),
		Some(_) => match std::fs::read(&request.path[1..]) {
			Ok(v) => create_response(buf, mhttp::Status::Ok, v, keep_alive),
			Err(e) => {
				use mhttp::Status::*;
				match e.kind() {
					ErrorKind::NotFound => create_response(buf, NotFound, NOT_FOUND, keep_alive),
					_ => {
						create_response(buf, InternalServerError, INTERNAL_SERVER_ERROR, keep_alive)
					}
				}
			}
		},
	}
}

fn bad_request(buf: &mut [u8], keep_alive: bool) -> (&[u8], Cow<'static, [u8]>, bool) {
	create_response(buf, mhttp::Status::BadRequest, BAD_REQUEST, keep_alive)
}

fn num_to_str(n: usize, buf: &mut [u8]) -> &str {
	let mut p = &mut buf[..];
	write!(p, "{}", n).unwrap();
	let d = p.len();
	core::str::from_utf8(&buf[..buf.len() - d]).unwrap()
}

fn create_response(
	buf: &mut [u8],
	status: mhttp::Status,
	body: impl Into<Cow<'static, [u8]>>,
	keep_alive: bool,
) -> (&[u8], Cow<'static, [u8]>, bool) {
	let mut l = [0; 20];
	let body = body.into();
	(
		mhttp::ResponseBuilder::new(buf, status)
			.unwrap()
			.add_header("content-length", num_to_str(body.len(), &mut l))
			.unwrap()
			.finish()
			.0,
		body,
		keep_alive,
	)
}
