#![feature(norostb)]
// FIXME figure out why rustc doesn't let us use data structures from an re-exported crate in
// stdlib
#![feature(rustc_private)]

use norostb_kernel::{io::Job, syscall};
use std::fs;
use std::io::{Read, Seek, Write};
use std::ptr::NonNull;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// TODO get disk from arguments
	let mut args = std::env::args().skip(1);
	let table_name = args.next().ok_or("expected table name")?;
	let disk = args.next().ok_or("expected disk path")?;

	let disk = loop {
		if let Ok(disk) = fs::OpenOptions::new().read(true).write(true).open(&disk) {
			break disk;
		}
		// TODO we probably should add a syscall to monitor the table list
		std::thread::yield_now();
	};

	let disk = driver_utils::io::BufBlock::new(disk);
	let fs =
		fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("failed to open filesystem");

	// Register new table of Streaming type
	let tbl = syscall::create_table(table_name.as_bytes(), syscall::TableType::Streaming).unwrap();

	let mut queries = driver_utils::Arena::new();
	let mut open_files = driver_utils::Arena::new();

	let mut buf = [0; 4096];
	let buf = &mut buf;

	loop {
		// Wait for events from the table
		let mut job = std::os::norostb::Job::default();
		job.buffer = NonNull::new(buf.as_mut_ptr());
		job.buffer_size = buf.len().try_into().unwrap();
		match std::os::norostb::take_job(tbl, &mut job) {
			Ok(()) => {}
			Err(_) => continue, // Timeout, probably...
		}

		match job.ty {
			Job::OPEN => {
				let path = std::str::from_utf8(&buf[..job.operation_size.try_into().unwrap()])
					.expect("what do?");
				if fs.root_dir().open_file(path).is_ok() {
					job.handle = open_files.insert((path.to_string(), 0u64));
				} else {
					match fs.root_dir().open_file(path) {
						Ok(_) => unreachable!(),
						Err(e) => dbg!(e),
					};
					todo!("how do I return an error?");
				}
			}
			Job::CREATE => {
				let path = std::str::from_utf8(&buf[..job.operation_size.try_into().unwrap()])
					.expect("what do?");
				if fs.root_dir().create_file(path).is_ok() {
					job.handle = open_files.insert((path.to_string(), 0u64));
				} else {
					match fs.root_dir().open_file(path) {
						Ok(_) => unreachable!(),
						Err(e) => dbg!(e),
					};
					todo!("how do I return an error?");
				}
			}
			Job::READ => {
				let (path, offset) = &open_files[job.handle];
				let mut file = fs.root_dir().open_file(path).unwrap();
				file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
				let l = file
					.read(&mut buf[..job.operation_size.try_into().unwrap()])
					.unwrap();
				job.operation_size = l.try_into().unwrap();
				open_files[job.handle].1 += u64::try_from(l).unwrap();
			}
			Job::WRITE => {
				let (path, offset) = &open_files[job.handle];
				let mut file = fs.root_dir().open_file(path).unwrap();
				file.seek(std::io::SeekFrom::Start(*offset)).unwrap();
				let l = file
					.write(&buf[..job.operation_size.try_into().unwrap()])
					.unwrap();
				job.operation_size = l.try_into().unwrap();
				open_files[job.handle].1 += u64::try_from(l).unwrap();
			}
			Job::QUERY => {
				let entries = fs
					.root_dir()
					.iter()
					.filter_map(|e| e.ok().map(|e| e.file_name()))
					.collect::<Vec<_>>();
				job.handle = queries.insert(entries);
			}
			Job::QUERY_NEXT => {
				match queries[job.handle].pop() {
					Some(f) => {
						buf[..f.len()].copy_from_slice(f.as_bytes());
						job.operation_size = f.len().try_into().unwrap();
					}
					None => {
						queries.remove(job.handle);
						job.operation_size = 0;
					}
				};
			}
			Job::SEEK => {
				todo!()
			}
			Job::CLOSE => {
				open_files.remove(job.handle);
				// The kernel does not expect a response.
				continue;
			}
			t => todo!("job type {}", t),
		}

		std::os::norostb::finish_job(tbl, &job).unwrap();
	}
}
