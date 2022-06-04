#![feature(norostb)]
#![feature(seek_stream_len)]

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

	let mut buf = [0; 4096];
	let buf = &mut buf;

	loop {
		use rt::io::Job;

		// Wait for events from the table
		let n = tbl.read(buf).unwrap();
		let (mut job, data) = Job::deserialize(&buf[..n]).unwrap();

		let len = match job.ty {
			Job::OPEN => {
				match str::from_utf8(data) {
					Ok("") => {
						let entries = fs
							.root_dir()
							.iter()
							.filter_map(|e| e.ok().map(|e| e.file_name()))
							.collect::<Vec<_>>();
						job.handle = objects.insert(Object::Query(entries, 0));
					}
					Ok(path) => match fs.root_dir().open_file(path) {
						Ok(_) => {
							job.handle = objects.insert(Object::File(path.to_string(), 0u64));
						}
						Err(e) => {
							job.result = (match e {
								fatfs::Error::NotFound => rt::Error::DoesNotExist,
								fatfs::Error::AlreadyExists => rt::Error::AlreadyExists,
								_ => rt::Error::Unknown,
							}) as _
						}
					},
					Err(_) => job.result = rt::Error::InvalidData as _,
				}
				0
			}
			Job::CREATE => {
				match str::from_utf8(buf) {
					Ok(path) => match fs.root_dir().create_file(path) {
						Ok(_) => job.handle = objects.insert(Object::File(path.to_string(), 0u64)),
						Err(e) => todo!("{:?}", e),
					},
					Err(_) => job.result = rt::Error::InvalidData as _,
				}
				0
			}
			Job::READ | Job::PEEK => {
				let len = u64::from_ne_bytes(data.try_into().unwrap());
				let d = &mut buf[job.as_ref().len()..];
				match &mut objects[job.handle] {
					Object::File(path, offset) => {
						let mut file = fs.root_dir().open_file(path).unwrap();
						file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
						let len = len.min(d.len() as u64) as usize;
						let l = file.read(&mut d[..len]).unwrap();
						if job.ty == Job::READ {
							*offset += u64::try_from(l).unwrap();
						}
						l
					}
					Object::Query(list, index) => match list.get(*index) {
						Some(f) => {
							d[..f.len()].copy_from_slice(f.as_bytes());
							if job.ty == Job::READ {
								*index += 1;
							}
							f.len()
						}
						None => 0,
					},
				}
			}
			Job::WRITE => match &mut objects[job.handle] {
				Object::File(path, offset) => {
					let mut file = fs.root_dir().open_file(path).unwrap();
					file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
					let l = file.write(data).unwrap();
					let l = u64::try_from(l).unwrap();
					*offset += l;
					let d = &mut buf[job.as_ref().len()..];
					d[..8].copy_from_slice(&l.to_ne_bytes());
					8
				}
				Object::Query(_, _) => {
					job.result = rt::Error::InvalidOperation as _;
					0
				}
			},
			Job::SEEK => {
				use rt::io::SeekFrom;
				let offt = u64::from_ne_bytes(data.try_into().unwrap());
				let d = &mut buf[job.as_ref().len()..];
				match &mut objects[job.handle] {
					Object::File(path, offset) => {
						match SeekFrom::try_from_raw(job.from_anchor, offt).unwrap() {
							SeekFrom::Start(n) => *offset = n,
							SeekFrom::Current(n) => *offset = offset.wrapping_add(n as u64),
							SeekFrom::End(n) => {
								let mut file = fs.root_dir().open_file(path).unwrap();
								let l = file.stream_len().unwrap();
								*offset = l.wrapping_add(n as u64);
							}
						}
						d[..8].copy_from_slice(&offset.to_ne_bytes());
						8
					}
					Object::Query(list, index) => {
						match SeekFrom::try_from_raw(job.from_anchor, offt).unwrap() {
							SeekFrom::Start(n) => *index = n as usize,
							SeekFrom::Current(n) => *index = index.wrapping_add(n as usize),
							SeekFrom::End(n) => *index = list.len().wrapping_sub(n as usize),
						}
						d[..8].copy_from_slice(&(*index as u64).to_ne_bytes());
						8
					}
				}
			}
			Job::CLOSE => {
				objects.remove(job.handle);
				// The kernel does not expect a response.
				continue;
			}
			t => todo!("job type {}", t),
		};

		buf[..job.as_ref().len()].copy_from_slice(job.as_ref());
		tbl.write(&buf[..job.as_ref().len() + len]).unwrap();
	}
}
