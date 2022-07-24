#![feature(norostb)]
#![feature(seek_stream_len)]

use driver_utils::os::stream_table::{Data, Request, Response, StreamTable};
use rt::io::Pow2Size;
use std::{
	fs,
	io::{Read, Seek, Write},
	str,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
	let tbl = {
		let buf = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 16 }).unwrap();
		StreamTable::new(&buf, Pow2Size(9))
	};
	rt::io::file_root()
		.unwrap()
		.create(table_name.as_bytes())
		.unwrap()
		.share(tbl.public())
		.unwrap();

	let mut objects = driver_utils::Arena::new();
	enum Object {
		File(String, u64),
		Query(Vec<String>, usize),
	}

	let mut buf = [0; 4096];
	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((handle, req)) = tbl.dequeue() {
			let (job_id, resp) = match req {
				Request::Open { job_id, path } => (
					job_id,
					if handle != rt::Handle::MAX {
						Response::Error(rt::Error::InvalidOperation)
					} else {
						let l = path.len();
						path.copy_to(0, &mut buf[..l]);
						path.manual_drop();
						match str::from_utf8(&buf[..l]) {
							Ok("") => {
								let entries = fs
									.root_dir()
									.iter()
									.filter_map(|e| e.ok().map(|e| e.file_name()))
									.collect::<Vec<_>>();
								Response::Handle(objects.insert(Object::Query(entries, 0)))
							}
							Ok(path) => match fs.root_dir().open_file(path) {
								Ok(_) => Response::Handle(
									objects.insert(Object::File(path.to_string(), 0u64)),
								),
								Err(e) => Response::Error(match e {
									fatfs::Error::NotFound => rt::Error::DoesNotExist,
									fatfs::Error::AlreadyExists => rt::Error::AlreadyExists,
									_ => rt::Error::Unknown,
								}),
							},
							Err(_) => Response::Error(rt::Error::InvalidData),
						}
					},
				),
				Request::Create { job_id, path } => (
					job_id,
					if handle != rt::Handle::MAX {
						Response::Error(rt::Error::InvalidOperation)
					} else {
						let l = path.len();
						path.copy_to(0, &mut buf[..l]);
						path.manual_drop();
						match str::from_utf8(&buf[..l]) {
							Ok(path) => match fs.root_dir().create_file(path) {
								Ok(_) => Response::Handle(
									objects.insert(Object::File(path.to_string(), 0u64)),
								),
								Err(e) => todo!("{:?}", e),
							},
							Err(_) => Response::Error(rt::Error::InvalidData),
						}
					},
				),
				Request::Read {
					peek,
					job_id,
					amount,
				} => (
					job_id,
					match &mut objects[handle] {
						Object::File(path, offset) => {
							let mut file = fs.root_dir().open_file(path).unwrap();
							file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
							let len = amount.min(4096) as usize;
							let len = file.read(&mut buf[..len]).unwrap();
							let data = tbl.alloc(len).expect("out of buffers");
							data.copy_from(0, &buf[..len]);
							if !peek {
								*offset += u64::try_from(len).unwrap();
							}
							Response::Data(data)
						}
						Object::Query(list, index) => {
							let f = match list.get(*index) {
								Some(f) => {
									if !peek {
										*index += 1;
									}
									f
								}
								None => "",
							};
							let data = tbl.alloc(f.len()).expect("out of buffers");
							data.copy_from(0, f.as_bytes());
							Response::Data(data)
						}
					},
				),
				Request::Write { job_id, data } => (
					job_id,
					match &mut objects[handle] {
						Object::File(path, offset) => {
							let l = data.len();
							data.copy_to(0, &mut buf[..l]);
							data.manual_drop();
							let mut file = fs.root_dir().open_file(path).unwrap();
							file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
							let l = file.write(&buf[..l]).unwrap();
							*offset += u64::try_from(l).unwrap();
							Response::Amount(l.try_into().unwrap())
						}
						Object::Query(_, _) => Response::Error(rt::Error::InvalidOperation),
					},
				),
				Request::Seek { job_id, from } => (job_id, {
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
							Response::Position(*offset)
						}
						Object::Query(list, index) => {
							match from {
								SeekFrom::Start(n) => *index = n as usize,
								SeekFrom::Current(n) => *index = index.wrapping_add(n as usize),
								SeekFrom::End(n) => *index = list.len().wrapping_sub(n as usize),
							}
							Response::Position(*index as _)
						}
					}
				}),
				Request::Close => {
					objects.remove(handle);
					continue;
				}
				Request::Share { .. } => todo!(),
				Request::Destroy { .. } => todo!(),
				Request::GetMeta { .. } => todo!(),
				Request::SetMeta { .. } => todo!(),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
	}
}
