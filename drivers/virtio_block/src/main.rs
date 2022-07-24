#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use driver_utils::os::stream_table::{Request, Response, StreamTable};
use rt::{io::Pow2Size, Handle};
use rt_default as _;
use virtio_block::Sector;

const SECTOR_SIZE: u32 = Sector::SIZE as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let file_root = rt::io::file_root().expect("no file root");
	let table_name = rt::args::Args::new()
		.skip(1)
		.next()
		.expect("expected table name");

	let dev_handle = {
		let s = b" 1af4:1001";
		let mut it = file_root.open(b"pci/info").unwrap();
		let mut buf = [0; 64];
		loop {
			let l = it.read(&mut buf).unwrap();
			assert!(l != 0, "device not found");
			let dev = &buf[..l];
			if dev.ends_with(s) {
				let mut path = Vec::from(*b"pci/");
				path.extend(&dev[..7]);
				break file_root.open(&path).unwrap();
			}
		}
	};

	let poll = dev_handle.open(b"poll").unwrap();
	let pci_config = dev_handle
		.map_object(None, rt::RWX::R, 0, usize::MAX)
		.unwrap()
		.0;

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let mut dev = {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					assert!(bar < 6);
					let mut s = *b"bar0";
					s[3] += bar;
					dev_handle
						.open(&s)
						.unwrap()
						.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
						.unwrap()
						.0
						.cast()
				};
				let dma_alloc = |size: usize, _align| -> Result<_, ()> {
					let (d, a, _) = driver_utils::dma::alloc_dma(size.try_into().unwrap()).unwrap();
					Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
				};

				let msix = virtio_block::Msix { queue: Some(0) };

				unsafe { virtio_block::BlockDevice::new(h, map_bar, dma_alloc, msix).unwrap() }
			}
			_ => unreachable!(),
		}
	};

	// Register new table of Streaming type
	let (tbl, dma_phys) = {
		let (dma, dma_phys) =
			driver_utils::dma::alloc_dma_object((1 << 16).try_into().unwrap()).unwrap();
		let tbl = StreamTable::new(&dma, Pow2Size(9), (1 << 12) - 1);
		file_root
			.create(table_name)
			.unwrap()
			.share(tbl.public())
			.unwrap();
		(tbl, dma_phys)
	};

	let mut data_handles = driver_utils::Arena::new();

	loop {
		let wait = || poll.read(&mut []).unwrap();

		let mut flush = false;
		while let Some((handle, req)) = tbl.dequeue() {
			let (job_id, resp) = match req {
				Request::Open { job_id, path } => {
					let r = if handle == Handle::MAX {
						if path.len() == 4 && {
							let mut buf = [0; 4];
							path.copy_to(0, &mut buf);
							buf == *b"data"
						} {
							Response::Handle(data_handles.insert(0))
						} else {
							Response::Error(rt::Error::InvalidData)
						}
					} else {
						Response::Error(rt::Error::InvalidOperation)
					};
					path.manual_drop();
					(job_id, r)
				}
				Request::Read {
					peek,
					job_id,
					amount,
				} => {
					(
						job_id,
						if handle == Handle::MAX {
							Response::Error(rt::Error::InvalidOperation)
						} else {
							// TODO how do we with unaligned reads/writes?
							assert!(amount % SECTOR_SIZE == 0);
							let amount = amount.min(1 << 13);
							let offset = data_handles[handle];

							let data = tbl
								.alloc(amount.try_into().unwrap())
								.expect("out of buffers");
							let sectors = data.blocks().map(|b| virtio::PhysRegion {
								base: virtio::PhysAddr::new(dma_phys + u64::from(b.0) * 512),
								size: 512,
							});

							let tk = unsafe { dev.read(sectors, offset).unwrap() };
							// TODO proper async
							while dev.poll_finished(|t| assert_eq!(t, tk)) != 1 {
								wait();
							}

							if !peek {
								data_handles[handle] += u64::from(amount / SECTOR_SIZE);
							}

							Response::Data(data)
						},
					)
				}
				Request::Write { job_id, data } => {
					// TODO ditto
					assert!(data.len() % Sector::SIZE == 0);
					let offset = data_handles[handle];

					let sectors = data.blocks().map(|b| virtio::PhysRegion {
						base: virtio::PhysAddr::new(dma_phys + u64::from(b.0) * 512),
						size: 512,
					});

					let tk = unsafe { dev.write(sectors, offset).unwrap() };
					// TODO proper async
					while dev.poll_finished(|t| assert_eq!(t, tk)) != 1 {
						wait();
					}
					let len = data.len();

					data.manual_drop();

					data_handles[handle] += u64::try_from(len / Sector::SIZE).unwrap();

					(job_id, Response::Amount(len.try_into().unwrap()))
				}
				Request::Seek { job_id, from } => {
					let offset = match from {
						rt::io::SeekFrom::Start(n) => n,
						_ => todo!(),
					};
					// TODO ditto
					assert!(offset % u64::from(SECTOR_SIZE) == 0);
					data_handles[handle] = offset / u64::from(SECTOR_SIZE);
					(job_id, Response::Position(offset))
				}
				Request::Close => {
					data_handles.remove(handle);
					// The kernel does not expect a response.
					continue;
				}
				Request::Create { job_id, path } => {
					path.manual_drop();
					(job_id, Response::Error(rt::Error::InvalidOperation))
				}
				Request::Share { .. } => todo!(),
				Request::GetMeta { .. } => todo!(),
				Request::SetMeta { .. } => todo!(),
				Request::Destroy { .. } => todo!(),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
		tbl.wait();
	}
}
