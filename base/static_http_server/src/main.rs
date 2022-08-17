#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use async_std::{
	eprintln,
	io::{Buf, Error, Read, Write},
	net::{Ipv4Addr, TcpListener, TcpStream},
	println,
};
use futures_util::{
	future::{self, Either},
	stream::{FuturesUnordered, StreamExt},
};
use rt_default as _;

const BAD_REQUEST: &[u8] = b"<!DOCTYPE html><h1>400 Bad Request</h1>";
const NOT_FOUND: &[u8] = b"<!DOCTYPE html><h1>404 Not Found</h1>";
const INTERNAL_SERVER_ERROR: &[u8] = b"<!DOCTYPE html><h1>500 Internal Server Error</h1>";

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	async_std::task::block_on(main())
}

async fn main() -> ! {
	eprintln!("creating listener").await;
	let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 80))
		.await
		.unwrap();
	eprintln!("accepting incoming connections").await;

	let mut accept = Box::pin(listener.accept());

	let do_client = |c: TcpStream| async move {
		loop {
			let (res, buf) = c.read(Vec::with_capacity(2048)).await;
			let mut headers = [""; 128];
			let (mut resp, keep_alive) = match res {
				Ok(0) => return, // Client closed the connection prematurely
				Ok(_) => match mhttp::RequestParser::parse(&buf, &mut headers) {
					Ok((req, _)) => handle_client(Vec::new(), req).await,
					Err(_) => bad_request(buf, false),
				},
				Err(_) => bad_request(buf, false),
			};
			let mut total = 0;
			while let (Ok(n), r) = c.write(resp.slice(total..)).await {
				resp = r.into_inner();
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
	let mut clients = FuturesUnordered::new();
	let mut finish_client = clients.next();

	loop {
		// Accept new clients
		match future::select(accept, finish_client).await {
			Either::Left((r, _f)) => {
				clients.push(do_client(r.unwrap().0));
				accept = Box::pin(listener.accept());
				// TODO will this work properly always?
				finish_client = clients.next();
			}
			Either::Right((r, a)) => {
				if r.is_none() {
					clients.push(do_client(a.await.unwrap().0));
					accept = Box::pin(listener.accept());
					finish_client = clients.next();
				} else {
					accept = a;
					finish_client = clients.next();
				}
			}
		}
	}
}

async fn handle_client<'a>(buf: Vec<u8>, request: mhttp::RequestParser<'_, '_>) -> (Vec<u8>, bool) {
	println!("{:?}", request.path).await;
	let keep_alive = request.header("connection") == Some("keep-alive");
	match request.header("host") {
		None => bad_request(buf, keep_alive),
		Some(_) if !request.path.starts_with("/") => bad_request(buf, keep_alive),
		Some(_) => {
			let path = match &request.path[1..] {
				"" => "index",
				p => p,
			};
			match async_std::fs::read(Vec::from(path)).await.0 {
				Ok(v) => create_response(
					buf,
					mhttp::Status::Ok,
					v.len(),
					|w| w.extend(&v),
					keep_alive,
				),
				Err(e) => {
					use mhttp::Status::*;
					match e {
						Error::DoesNotExist => create_response(
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

fn num_to_str(mut n: usize, buf: &mut [u8]) -> &str {
	let mut l = 0;
	for w in buf.iter_mut().rev() {
		*w = (n % 10) as u8 + b'0';
		n /= 10;
		l += 1;
		if n == 0 {
			break;
		}
	}
	core::str::from_utf8(&buf[buf.len() - l..]).unwrap()
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
