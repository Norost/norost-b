#![feature(norostb)]
#![feature(seek_stream_len)]

use {
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	rt::io::Pow2Size,
	std::{
		fs,
		io::{Read, Seek, Write},
		str,
	},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args().skip(1);
	let table_name = args.next().ok_or("expected table name")?;

	let disk = rt::args::handle(b"data").expect("data object undefined");
	let share = rt::args::handle(b"share").expect("share object undefined");

	use std::os::norostb::io::FromHandle;
	let disk = unsafe { fs::File::from_handle(disk.as_raw()) };

	let disk = driver_utils::io::BufBlock::new(disk);
	let fs =
		fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("failed to open filesystem");

	// Create a new table.
	let tbl = {
		let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 16 }).unwrap();
		StreamTable::new(&buf, Pow2Size(9), (1 << 12) - 1)
	};
	share
		.create(table_name.as_bytes())
		.unwrap()
		.share(tbl.public())
		.unwrap();

	let mut objects = driver_utils::Arena::new();
	enum Object {
		File(String, u64),
		#[allow(dead_code)]
		Dir(String, u64),
		Query(Vec<String>, usize),
	}

	let mut buf = [0; 4096];
	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((handle, job_id, req)) = tbl.dequeue() {
			let resp = match req {
				Request::Open { path } => {
					if handle != rt::Handle::MAX {
						Response::Error(rt::Error::InvalidOperation)
					} else {
						let l = path.len();
						path.copy_to(0, &mut buf[..l]);
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
					}
				}
				Request::Create { path } => {
					if handle != rt::Handle::MAX {
						Response::Error(rt::Error::InvalidOperation)
					} else {
						let l = path.len();
						path.copy_to(0, &mut buf[..l]);
						match str::from_utf8(&buf[..l]) {
							Ok("") | Ok("/") => Response::Error(rt::Error::AlreadyExists),
							Ok(path) if path.bytes().last() == Some(b'/') => {
								match fs.root_dir().create_dir(&path[..path.len() - 1]) {
									Ok(_) => Response::Handle(
										objects.insert(Object::File(path.to_string(), 0u64)),
									),
									Err(e) => todo!("{:?}", e),
								}
							}
							Ok(path) => match fs.root_dir().create_file(path) {
								Ok(mut f) => {
									f.truncate().unwrap();
									let h = objects.insert(Object::File(path.to_string(), 0u64));
									Response::Handle(h)
								}
								Err(e) => todo!("{:?}", e),
							},
							Err(_) => Response::Error(rt::Error::InvalidData),
						}
					}
				}
				Request::Read { amount } => match &mut objects[handle] {
					Object::File(path, offset) => {
						let mut file = fs.root_dir().open_file(path).unwrap();
						file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
						let len = amount.min(4096) as usize;
						let len = file.read(&mut buf[..len]).unwrap();
						let data = tbl.alloc(len).expect("out of buffers");
						data.copy_from(0, &buf[..len]);
						*offset += u64::try_from(len).unwrap();
						Response::Data(data)
					}
					Object::Dir(..) => todo!(),
					Object::Query(list, index) => {
						let f = match list.get(*index) {
							Some(f) => {
								*index += 1;
								f
							}
							None => "",
						};
						let data = tbl.alloc(f.len()).expect("out of buffers");
						data.copy_from(0, f.as_bytes());
						Response::Data(data)
					}
				},
				Request::Write { data } => match &mut objects[handle] {
					Object::File(path, offset) => {
						let l = data.len();
						data.copy_to(0, &mut buf[..l]);
						let mut file = fs.root_dir().open_file(path).unwrap();
						file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
						let l = file.write(&buf[..l]).unwrap();
						*offset += u64::try_from(l).unwrap();
						Response::Amount(l.try_into().unwrap())
					}
					Object::Dir(..) | Object::Query(..) => {
						Response::Error(rt::Error::InvalidOperation)
					}
				},
				Request::Seek { from } => {
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
						Object::Dir(..) => todo!(),
						Object::Query(list, index) => {
							match from {
								SeekFrom::Start(n) => *index = n as usize,
								SeekFrom::Current(n) => *index = index.wrapping_add(n as usize),
								SeekFrom::End(n) => *index = list.len().wrapping_sub(n as usize),
							}
							Response::Position(*index as _)
						}
					}
				}
				Request::Close => {
					objects
						.remove(handle)
						.expect("todo: deal with Handle::MAX close");
					continue;
				}
				Request::Share { .. } => todo!(),
				Request::Destroy { .. } => todo!(),
				Request::GetMeta { property } => match &*property.get(&mut buf) {
					b"fs/type" => {
						let t: Option<&[_]> = if handle == rt::Handle::MAX {
							Some(b"dir")
						} else {
							match &objects[handle] {
								Object::File(..) => Some(b"file"),
								Object::Dir(..) => Some(b"dir"),
								Object::Query(..) => None,
							}
						};
						if let Some(t) = t {
							let d = tbl.alloc(t.len()).expect("out of buffers");
							d.copy_from(0, t);
							Response::Data(d)
						} else {
							Response::Error(rt::Error::DoesNotExist)
						}
					}
					_ => Response::Error(rt::Error::DoesNotExist),
				},
				Request::SetMeta { .. } => todo!(),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
	}
}
