#![feature(norostb)]

use core::ptr::NonNull;
use norostb_kernel::{io::Job, syscall};
use norostb_rt as rt;
use std::fs;
use std::os::norostb::prelude::*;
use virtio_block::Sector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let table_name = std::env::args()
		.skip(1)
		.next()
		.ok_or("expected table name")?;

	let dev_handle = {
		let s = b" 1af4:1001";
		let mut it = fs::read_dir("pci/info").unwrap().map(Result::unwrap);
		loop {
			let dev = it.next().unwrap().path().into_os_string().into_vec();
			if dev.ends_with(s) {
				let mut path = Vec::from(*b"pci/");
				path.extend(&dev[..7]);
				break fs::File::open(std::ffi::OsString::from_vec(path))
					.unwrap()
					.into_handle();
			}
		}
	};
	let pci_config = syscall::map_object(dev_handle, None, 0, usize::MAX).unwrap();

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let mut dev = {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					syscall::map_object(dev_handle, None, (bar + 1).into(), usize::MAX)
						.unwrap()
						.cast()
				};
				let dma_alloc = |size, _align| -> Result<_, ()> {
					let (d, _) = syscall::alloc_dma(None, size).unwrap();
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
				};

				let msix = virtio_block::Msix { queue: Some(0) };

				unsafe { virtio_block::BlockDevice::new(h, map_bar, dma_alloc, msix).unwrap() }
			}
			_ => unreachable!(),
		}
	};

	// Register new table of Streaming type
	let tbl = rt::io::file_root()
		.unwrap()
		.create(table_name.as_bytes())
		.unwrap();

	let (sectors, _) = syscall::alloc_dma(None, 4096).unwrap();
	let sectors_phys = virtio::PhysRegion {
		base: virtio::PhysAddr::new(
			syscall::physical_address(sectors)
				.unwrap()
				.try_into()
				.unwrap(),
		),
		size: 4096,
	};
	let sectors = unsafe {
		core::slice::from_raw_parts_mut(sectors.cast::<Sector>().as_ptr(), 4096 / Sector::SIZE)
	};

	let mut queries = driver_utils::Arena::new();
	let mut data_handles = driver_utils::Arena::new();

	let mut buf = [0; 4096];
	let buf = &mut buf;

	enum QueryState {
		Data,
		Info,
	}

	loop {
		// Wait for events from the table
		let mut job = rt::io::Job::default();
		job.buffer = NonNull::new(buf.as_mut_ptr());
		job.buffer_size = buf.len().try_into().unwrap();
		tbl.take_job(&mut job).unwrap();

		let wait = || {
			// FIXME this API is fundamentally broken as it's subject to race conditions.
			// There are two things missing to make this particular API useable:
			//
			// 1) Acknowledgement that the server has received the poll request
			// 2) Receiving the event itself.
			//
			// 1) is required to prevent the race condition. 2) is so we know when to continue.
			//
			// Alternatively, some kind of "event" object should be created. The server then
			// knows to keep track of events which will directly prevent race conditions from
			// occuring as events are continuously collected.
			//rt::io::poll(dev_handle).unwrap();
		};

		match job.ty {
			Job::OPEN => {
				job.handle = data_handles.insert(0);
			}
			Job::READ => {
				let offset = data_handles[job.handle];
				let sector = offset / u64::try_from(Sector::SIZE).unwrap();
				let offset = offset % u64::try_from(Sector::SIZE).unwrap();
				let offset = u16::try_from(offset).unwrap();

				unsafe {
					dev.read(sectors_phys, sector, wait).unwrap();
				}

				let size = job.operation_size.min(
					(Sector::slice_as_u8(sectors).len() - usize::from(offset))
						.try_into()
						.unwrap(),
				);

				job.operation_size = size;
				data_handles[job.handle] = u64::from(offset) + u64::from(job.operation_size);

				let size = usize::try_from(size).unwrap();
				let offset = usize::from(offset);
				buf[..size].copy_from_slice(&Sector::slice_as_u8(&sectors)[offset..][..size]);
			}
			Job::WRITE => {
				let offset = data_handles[job.handle];
				let sector = offset / u64::try_from(Sector::SIZE).unwrap();
				let offset = offset % u64::try_from(Sector::SIZE).unwrap();
				let offset = u16::try_from(offset).unwrap();

				unsafe {
					dev.read(sectors_phys, sector, wait).unwrap();
				}

				let data = &buf[..job.operation_size as usize];
				Sector::slice_as_u8_mut(sectors)[offset.into()..][..data.len()]
					.copy_from_slice(data);

				unsafe {
					dev.write(sectors_phys, sector, wait).unwrap();
				}

				data_handles[job.handle] = u64::from(offset) + u64::from(job.operation_size);
			}
			Job::QUERY => {
				job.handle = queries.insert(Some(QueryState::Data));
			}
			Job::QUERY_NEXT => {
				match queries[job.handle] {
					Some(QueryState::Data) => {
						buf[..4].copy_from_slice(b"data");
						job.operation_size = 4;
						queries[job.handle] = Some(QueryState::Info);
					}
					Some(QueryState::Info) => {
						buf[..4].copy_from_slice(b"info");
						job.operation_size = 4;
						queries[job.handle] = None;
					}
					None => {
						queries.remove(job.handle);
						job.operation_size = 0;
					}
				};
			}
			Job::SEEK => {
				use norostb_kernel::io::SeekFrom;
				let from = SeekFrom::try_from_raw(job.from_anchor, job.from_offset).unwrap();
				let offset = match from {
					SeekFrom::Start(n) => n,
					_ => todo!(),
				};
				data_handles[job.handle] = offset;
			}
			Job::CLOSE => {
				data_handles.remove(job.handle);
				// The kernel does not expect a response.
				continue;
			}
			t => todo!("job type {}", t),
		}

		tbl.finish_job(&job).unwrap();
	}
}
