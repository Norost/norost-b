#![feature(norostb)]
// FIXME figure out why rustc doesn't let us use data structures from an re-exported crate in
// stdlib
#![feature(rustc_private)]

use norostb_kernel::{self as kernel, io, syscall};
use std::fs;
use std::io::{Read, Seek, Write};
use std::ptr::NonNull;

fn main() {
	// TODO get disk from arguments

	let disk = fs::OpenOptions::new()
		.read(true)
		.write(true)
		.open("virtio-blk/disk/0")
		.expect("failed to open disk");

	struct IoMonitor<P: AsRef<str>, T> {
		prefix: P,
		io: T,
	};
	impl<P: AsRef<str>, T: Read> Read for IoMonitor<P, T> {
		fn read(&mut self, data: &mut [u8]) -> std::io::Result<usize> {
			let r = self.io.read(data);
			match &r {
				Ok(l) => eprintln!(
					"[{}] Read {} bytes into {} byte buffer",
					self.prefix.as_ref(),
					l,
					data.len()
				),
				Err(e) => eprintln!(
					"[{}] Failed to read into {} byte buffer: {:?}",
					self.prefix.as_ref(),
					data.len(),
					e
				),
			}
			r
		}
	}
	impl<P: AsRef<str>, T: Write> Write for IoMonitor<P, T> {
		fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
			let r = self.io.write(data);
			match &r {
				Ok(l) => eprintln!(
					"[{}] Write {} bytes of {} byte buffer",
					self.prefix.as_ref(),
					l,
					data.len()
				),
				Err(e) => eprintln!(
					"[{}] Failed to write {} byte buffer: {:?}",
					self.prefix.as_ref(),
					data.len(),
					e
				),
			}
			r
		}

		fn flush(&mut self) -> std::io::Result<()> {
			let r = self.io.flush();
			match &r {
				Ok(()) => eprintln!("[{}] Flushed", self.prefix.as_ref()),
				Err(e) => eprintln!("[{}] Failed to flush: {:?}", self.prefix.as_ref(), e),
			}
			r
		}
	}
	impl<P: AsRef<str>, T: Seek> Seek for IoMonitor<P, T> {
		fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
			let r = self.io.seek(from);
			match &r {
				Ok(l) => eprintln!(
					"[{}] Seeked to {:?}, now at {}",
					self.prefix.as_ref(),
					from,
					l
				),
				Err(e) => eprintln!(
					"[{}] Failed to seek to {:?}: {:?}",
					self.prefix.as_ref(),
					from,
					e
				),
			}
			r
		}
	}

	/*
	let disk = IoMonitor {
		prefix: "disk",
		io: disk,
	};
	*/
	let disk = fscommon::BufStream::new(disk);
	/*
	let disk = IoMonitor {
		prefix: "fat",
		io: disk,
	};
	*/
	let fs =
		fatfs::FileSystem::new(disk, fatfs::FsOptions::new()).expect("failed to open filesystem");

	dbg!(fs.stats());

	// Register new table of Streaming type
	let tbl = syscall::create_table(b"fat", syscall::TableType::Streaming).unwrap();

	let mut query_id_counter = 0u32;
	let mut queries = std::collections::BTreeMap::new();

	let mut buf = [0; 4096];
	let buf = &mut buf;

	loop {
		// Wait for events from the table
		let mut job = std::os::norostb::Job::default();
		job.buffer = NonNull::new(buf.as_mut_ptr());
		job.buffer_size = buf.len().try_into().unwrap();
		if std::os::norostb::take_job(tbl, &mut job).is_err() {
			std::thread::sleep(std::time::Duration::from_millis(100));
			continue;
		}

		match job.ty {
			syscall::Job::OPEN => {
				dbg!(std::str::from_utf8(
					&buf[..job.operation_size.try_into().unwrap()]
				));
				todo!()
			}
			syscall::Job::CREATE => {
				todo!()
			}
			syscall::Job::READ => {
				todo!()
			}
			syscall::Job::WRITE => {
				todo!()
			}
			syscall::Job::QUERY => {
				let entries = fs
					.root_dir()
					.iter()
					.filter_map(|e| e.ok().map(|e| e.file_name()))
					.collect::<Vec<_>>();
				queries.insert(query_id_counter, entries);
				job.query_id = query_id_counter;
				query_id_counter += 1;
			}
			syscall::Job::QUERY_NEXT => {
				match queries.get_mut(&job.query_id).and_then(|v| v.pop()) {
					Some(f) => {
						buf[..f.len()].copy_from_slice(f.as_bytes());
						job.operation_size = f.len().try_into().unwrap();
					}
					None => {
						queries.remove(&job.query_id);
						job.operation_size = 0;
					}
				};
			}
			syscall::Job::SEEK => {
				todo!()
			}
			t => todo!("job type {}", t),
		}

		std::os::norostb::finish_job(tbl, &job).unwrap();
	}
}
