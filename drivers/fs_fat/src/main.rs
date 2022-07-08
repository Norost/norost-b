#![feature(norostb)]
#![feature(seek_stream_len)]

use driver_utils::io::queue::stream::Job;
use norostb_rt as rt;
use std::{
	fs,
	io::{Read, Seek, Write},
	str,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// TODO get disk from arguments
	let mut args = std::env::args().skip(1);
	let table_name = args.next().ok_or("expected table name")?;
	let disk = args.next().ok_or("expected disk path")?;

	let disk = fs::OpenOptions::new()
		.read(true)
		.write(true)
		.open(&disk)
		.expect("disk not found");

	let disk = driver_utils::io::BufBlock::new(disk);
	let fs =
		fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("failed to open filesystem");

	// Create a new table.
	let tbl = rt::io::file_root()
		.unwrap()
		.create(table_name.as_bytes())
		.unwrap();

	let mut objects = driver_utils::Arena::new();
	enum Object {
		File(String, u64),
		Query(Vec<String>, usize),
	}

	let mut buf = Vec::new();

	loop {
		// Wait for events from the table
		buf.resize(4096, 0);
		let n = tbl.read(&mut buf).unwrap();
		buf.resize(n, 0);

		buf = match Job::deserialize(&buf).unwrap() {
			Job::Open {
				handle,
				job_id,
				path,
			} => if handle != rt::Handle::MAX {
				Job::reply_error_clear(buf, job_id, rt::Error::InvalidOperation)
			} else {
				match str::from_utf8(path) {
					Ok("") => {
						let entries = fs
							.root_dir()
							.iter()
							.filter_map(|e| e.ok().map(|e| e.file_name()))
							.collect::<Vec<_>>();
						let handle = objects.insert(Object::Query(entries, 0));
						Job::reply_open_clear(buf, job_id, handle)
					}
					Ok(path) => match fs.root_dir().open_file(path) {
						Ok(_) => {
							let handle = objects.insert(Object::File(path.to_string(), 0u64));
							Job::reply_open_clear(buf, job_id, handle)
						}
						Err(e) => Job::reply_error_clear(
							buf,
							job_id,
							match e {
								fatfs::Error::NotFound => rt::Error::DoesNotExist,
								fatfs::Error::AlreadyExists => rt::Error::AlreadyExists,
								_ => rt::Error::Unknown,
							},
						),
					},
					Err(_) => Job::reply_error_clear(buf, job_id, rt::Error::InvalidData),
				}
			}
			.unwrap(),
			Job::Create {
				handle,
				job_id,
				path,
			} => if handle != rt::Handle::MAX {
				Job::reply_error_clear(buf, job_id, rt::Error::InvalidOperation)
			} else {
				match str::from_utf8(path) {
					Ok(path) => match fs.root_dir().create_file(path) {
						Ok(_) => {
							let handle = objects.insert(Object::File(path.to_string(), 0u64));
							Job::reply_create_clear(buf, job_id, handle)
						}
						Err(e) => todo!("{:?}", e),
					},
					Err(_) => Job::reply_error_clear(buf, job_id, rt::Error::InvalidData),
				}
			}
			.unwrap(),
			Job::Read {
				peek,
				handle,
				job_id,
				length,
			} => match &mut objects[handle] {
				Object::File(path, offset) => {
					let mut file = fs.root_dir().open_file(path).unwrap();
					file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
					let len = length.min(4096) as usize;
					Job::reply_read_clear(buf, job_id, peek, |d| {
						let og_len = d.len();
						d.resize(og_len + len, 0);
						let l = file.read(&mut d[og_len..]).unwrap();
						d.resize(og_len + l, 0);
						if !peek {
							*offset += u64::try_from(l).unwrap();
						}
						Ok(())
					})
				}
				Object::Query(list, index) => match list.get(*index) {
					Some(f) => Job::reply_read_clear(buf, job_id, peek, |d| {
						d.extend(f.as_bytes());
						if !peek {
							*index += 1;
						}
						Ok(())
					}),
					None => Job::reply_read_clear(buf, job_id, peek, |_| Ok(())),
				},
			}
			.unwrap(),
			Job::Write {
				handle,
				job_id,
				data,
			} => match &mut objects[handle] {
				Object::File(path, offset) => {
					let mut file = fs.root_dir().open_file(path).unwrap();
					file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
					let l = file.write(data).unwrap();
					let l = u64::try_from(l).unwrap();
					*offset += l;
					Job::reply_write_clear(buf, job_id, l)
				}
				Object::Query(_, _) => {
					Job::reply_error_clear(buf, job_id, rt::Error::InvalidOperation)
				}
			}
			.unwrap(),
			Job::Seek {
				handle,
				job_id,
				from,
			} => {
				use rt::io::SeekFrom;
				match &mut objects[handle] {
					Object::File(path, offset) => {
						match from {
							SeekFrom::Start(n) => *offset = n,
							SeekFrom::Current(n) => *offset = offset.wrapping_add(n as u64),
							SeekFrom::End(n) => {
								let mut file = fs.root_dir().open_file(path).unwrap();
								let l = file.stream_len().unwrap();
								*offset = l.wrapping_add(n as u64);
							}
						}
						Job::reply_seek_clear(buf, job_id, *offset)
					}
					Object::Query(list, index) => {
						match from {
							SeekFrom::Start(n) => *index = n as usize,
							SeekFrom::Current(n) => *index = index.wrapping_add(n as usize),
							SeekFrom::End(n) => *index = list.len().wrapping_sub(n as usize),
						}
						Job::reply_seek_clear(buf, job_id, *index as u64)
					}
				}
				.unwrap()
			}
			Job::Close { handle } => {
				objects.remove(handle);
				// The kernel does not expect a response.
				continue;
			}
			Job::Share { .. } => todo!(),
		};
		tbl.write(&buf).unwrap();
	}
}
