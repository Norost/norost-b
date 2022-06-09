use futures::stream::Stream;
use std::{
	future::Future,
	io::{ErrorKind, Write},
	pin::Pin,
	task::{Context, Poll},
};

const BAD_REQUEST: &[u8] = b"<!DOCTYPE html><h1>400 Bad Request</h1>";
const NOT_FOUND: &[u8] = b"<!DOCTYPE html><h1>404 Not Found</h1>";
const INTERNAL_SERVER_ERROR: &[u8] = b"<!DOCTYPE html><h1>500 Internal Server Error</h1>";

fn main() {
	eprintln!("creating listener");
	let listener = rt::io::net_root()
		.unwrap()
		.create(b"default/tcp/listen/80")
		.unwrap();
	eprintln!("accepting incoming connections");

	let do_accept = move |s| rt::io::open(listener.as_raw(), s, 0);
	let mut accept = do_accept(Vec::from(*b"accept"));

	let mut bufs = Vec::new();

	let do_client = |c: rt::Object, buf: Vec<_>, buf2: Vec<_>| async move {
		loop {
			let mut headers = [""; 128];
			let (mut resp, keep_alive) = match rt::io::read(c.as_raw(), buf, 2048).await {
				Ok(b) if b.is_empty() => return, // Client closed the connection prematurely
				Ok(b) => match mhttp::RequestParser::parse(&b, &mut headers) {
					Ok((req, _)) => handle_client(buf2, req),
					Err(_) => bad_request(buf2, false),
				},
				Err(_) => bad_request(buf2, false),
			};
			let mut total = 0;
			while let Ok((r, n)) = rt::io::write(c.as_raw(), resp, total).await {
				resp = r;
				if n == 0 {
					return; // Client closed the connection prematurely
				}
				total += n;
				if total == resp.len() {
					break; // We wrote all data
				}
			}
			if !keep_alive {
				break;
			}
			break; // TODO
		}
	};
	let mut clients = futures::stream::FuturesUnordered::new();
	let mut cx = Context::from_waker(futures::task::noop_waker_ref());

	loop {
		// Accept new clients
		if let Poll::Ready(r) = Pin::new(&mut accept).poll(&mut cx) {
			let (accept_str, c) = r.unwrap();
			let (buf, buf2) = bufs.pop().unwrap_or_else(|| (Vec::new(), Vec::new()));
			clients.push(do_client(rt::Object::from_raw(c), buf, buf2));
			accept = do_accept(accept_str);
		};

		// Handle currently connected clients
		if !clients.is_empty() {
			while let Poll::Ready(Some(())) = Pin::new(&mut clients).poll_next(&mut cx) {}
		}

		rt::io::poll_queue_and_wait();
	}
}

fn handle_client<'a>(buf: Vec<u8>, request: mhttp::RequestParser) -> (Vec<u8>, bool) {
	println!("{:?}", request.path);
	let keep_alive = request.header("connection") == Some("keep-alive");
	match request.header("host") {
		None => bad_request(buf, keep_alive),
		Some(_) if !request.path.starts_with("/") => bad_request(buf, keep_alive),
		Some(_) => {
			let path = match &request.path[1..] {
				"" => "index",
				p => p,
			};
			match std::fs::read(path) {
				Ok(v) => create_response(
					buf,
					mhttp::Status::Ok,
					v.len(),
					|w| w.extend(&v),
					keep_alive,
				),
				Err(e) => {
					use mhttp::Status::*;
					match e.kind() {
						ErrorKind::NotFound => create_response(
							buf,
							NotFound,
							NOT_FOUND.len(),
							|v| v.extend(NOT_FOUND),
							keep_alive,
						),
						_ => create_response(
							buf,
							InternalServerError,
							INTERNAL_SERVER_ERROR.len(),
							|v| v.extend(INTERNAL_SERVER_ERROR),
							keep_alive,
						),
					}
				}
			}
		}
	}
}

fn bad_request(buf: Vec<u8>, keep_alive: bool) -> (Vec<u8>, bool) {
	create_response(
		buf,
		mhttp::Status::BadRequest,
		BAD_REQUEST.len(),
		|v| v.extend(BAD_REQUEST),
		keep_alive,
	)
}

fn num_to_str(n: usize, buf: &mut [u8]) -> &str {
	let mut p = &mut buf[..];
	write!(p, "{}", n).unwrap();
	let d = p.len();
	core::str::from_utf8(&buf[..buf.len() - d]).unwrap()
}

fn create_response(
	mut buf: Vec<u8>,
	status: mhttp::Status,
	body_len: usize,
	body: impl FnOnce(&mut Vec<u8>),
	keep_alive: bool,
) -> (Vec<u8>, bool) {
	let mut l = [0; 20];
	let mut h = [0; 256];
	let h = mhttp::ResponseBuilder::new(&mut h, status)
		.unwrap()
		.add_header("content-length", num_to_str(body_len, &mut l))
		.unwrap()
		.add_header("connection", "close") // TODO
		.unwrap()
		.finish()
		.0;
	buf.extend(h);
	body(&mut buf);
	(buf, keep_alive)
}
