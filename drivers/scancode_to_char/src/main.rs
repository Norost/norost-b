#![feature(norostb)]

use norostb_kernel::{error::Error, io, syscall};
use std::collections::VecDeque;
use std::os::norostb::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args_os().skip(1);
	let table = args.next().ok_or("expected table path")?;
	let input = args.next().ok_or("expected input object path")?;

	// Create I/O queue with two entries: one for a job and one for reading
	let request_p2size = 1;
	let response_p2size = 1;
	let io_queue = syscall::create_io_queue(None, request_p2size, response_p2size).unwrap();
	let mut io_queue = io::Queue {
		requests_mask: (1 << request_p2size) - 1,
		responses_mask: (1 << response_p2size) - 1,
		base: io_queue.cast(),
	};

	// Create a table
	let table = std::fs::File::create(table)?.into_handle();

	// Open input
	let input = std::fs::File::open(input)?.into_handle();

	// Enqueue initial requests
	const READ: u64 = 0;
	const TAKE_JOB: u64 = 1;
	const FINISH_JOB: u64 = 2;

	let mut scancode_buf = [0; 4];
	let scancode_buf = &mut scancode_buf;
	let mut job_buf = [0; 512];
	let job_buf = &mut job_buf;

	unsafe {
		io_queue
			.enqueue_request(io::Request::read(READ, input, scancode_buf))
			.unwrap();
		io_queue
			.enqueue_request(io::Request::read(TAKE_JOB, table, job_buf))
			.unwrap();
		syscall::process_io_queue(Some(io_queue.base.cast())).unwrap();
		syscall::wait_io_queue(Some(io_queue.base.cast())).unwrap();
	}

	let mut char_buf = VecDeque::new();

	let mut readers = driver_utils::Arena::new();

	let mut pending_read = None;

	let mut shifts = 0;

	loop {
		while let Ok(resp) = unsafe { io_queue.dequeue_response() } {
			match resp.user_data {
				READ => {
					use scancodes::{Event, ScanCode};
					assert_eq!(resp.value, 4, "incomplete scancode");
					let chr = match Event::try_from(*scancode_buf).unwrap() {
						Event::Press(ScanCode::LeftShift) | Event::Press(ScanCode::RightShift) => {
							shifts += 1;
							None
						}
						Event::Release(ScanCode::LeftShift)
						| Event::Release(ScanCode::RightShift) => {
							shifts -= 1;
							None
						}
						Event::Press(s) => match s {
							ScanCode::Backspace => Some(0x7f), // DEL
							ScanCode::Enter => Some(b'\n'),
							ScanCode::ForwardSlash => Some(b'/'),
							ScanCode::BackSlash => Some(b'\\'),
							ScanCode::Colon => Some(b':'),
							ScanCode::Semicolon => Some(b';'),
							ScanCode::Comma => Some(b','),
							ScanCode::Dot => Some(b'.'),
							ScanCode::SingleQuote => Some(b'\''),
							ScanCode::DoubleQuote => Some(b'"'),
							ScanCode::Space => Some(b' '),
							ScanCode::Minus if shifts == 0 => Some(b'-'),
							ScanCode::Minus if shifts > 0 => Some(b'_'),
							s => s
								.alphabet_to_char()
								.or_else(|| s.bracket_to_char())
								.or_else(|| s.number_to_char())
								.map(|c| {
									if shifts > 0 {
										c.to_ascii_uppercase() as u8
									} else {
										c as u8
									}
								}),
						},
						Event::Release(_) => None,
					};
					if let Some(chr) = chr {
						if let Some(job_id) = pending_read.take() {
							let job = io::Job {
								ty: io::Job::READ,
								job_id,
								..Default::default()
							};
							job_buf[job.as_ref().len()] = chr;
							job_buf[..job.as_ref().len()].copy_from_slice(job.as_ref());
							let b = &job_buf[..job.as_ref().len() + 1];
							unsafe {
								io_queue
									.enqueue_request(io::Request::write(FINISH_JOB, table, b))
									.unwrap();
							}
						} else {
							char_buf.push_back(chr);
						}
					}
					unsafe {
						io_queue
							.enqueue_request(io::Request::read(READ, input, scancode_buf))
							.unwrap();
					}
				}
				TAKE_JOB => {
					let data = &job_buf[..resp.value as usize];
					let (mut job, data) = io::Job::deserialize(data).unwrap();
					assert_eq!(job.result, 0);
					let len = match job.ty {
						io::Job::OPEN => {
							if data == b"stream" {
								job.handle = readers.insert(());
							} else {
								job.result = Error::InvalidObject as i16;
							}
							0
						}
						io::Job::CLOSE => {
							readers.remove(job.handle).unwrap();
							// The kernel does not expect a response
							unsafe {
								io_queue
									.enqueue_request(io::Request::read(TAKE_JOB, table, job_buf))
									.unwrap();
							}
							continue;
						}
						io::Job::READ => {
							// Ensure the handle is valid.
							readers[job.handle];
							let d = &mut job_buf[job.as_ref().len()..];
							let mut l = 0;
							for w in d.iter_mut() {
								if let Some(r) = char_buf.pop_front() {
									*w = r;
									l += 1;
								} else {
									break;
								}
							}
							if l == 0 {
								// There is currently no data, so delay a response until there is
								// data. Don't enqueue a new TAKE_JOB either so we have a slot
								// free for when we can send a reply
								pending_read = Some(job.job_id);
								continue;
							}
							l
						}
						_ => {
							job.result = Error::InvalidOperation as i16;
							0
						}
					};
					job_buf[..job.as_ref().len()].copy_from_slice(job.as_ref());
					let b = &job_buf[..job.as_ref().len() + len];
					unsafe {
						io_queue
							.enqueue_request(io::Request::write(FINISH_JOB, table, b))
							.unwrap();
					}
				}
				FINISH_JOB => unsafe {
					norostb_kernel::error::result(resp.value).expect("error finishing job");
					io_queue
						.enqueue_request(io::Request::read(TAKE_JOB, table, job_buf))
						.unwrap();
				},
				ud => panic!("invalid user data: {}", ud),
			}
		}
		syscall::process_io_queue(Some(io_queue.base.cast())).unwrap();
		syscall::wait_io_queue(Some(io_queue.base.cast())).unwrap();
	}
}
