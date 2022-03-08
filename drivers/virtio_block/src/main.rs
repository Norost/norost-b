use norostb_kernel::{self as kernel, syscall};

use core::ptr::NonNull;

fn main() {
	println!("Hello, world! from Rust");

	let mut id = None;
	let mut dev = None;
	println!("iter tables");
	'found_dev: while let Some((i, inf)) = syscall::next_table(id) {
		println!("table: {:?} -> {:?}", i, core::str::from_utf8(inf.name()));
		if inf.name() == b"pci" {
			let tags: [syscall::Slice<u8>; 2] =
				[b"vendor-id:1af4".into(), b"device-id:1001".into()];
			let h = syscall::query_table(i, None, &tags).unwrap();
			println!("{:?}", h);
			let mut buf = [0; 256];
			let mut obj = syscall::ObjectInfo::new(&mut buf);
			while let Ok(()) = syscall::query_next(h, &mut obj) {
				println!("{:#?}", &obj);
				dev = Some((i, obj.id));
				break 'found_dev;
			}
		}
		id = Some(i);
	}

	let (tbl, dev) = dev.unwrap();
	let handle = syscall::open_object(tbl, dev).unwrap();

	let pci_config = NonNull::new(0x1000_0000 as *mut _);
	let pci_config = syscall::map_object(handle, pci_config, 0, usize::MAX).unwrap();

	println!("handle: {:?}", handle);

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let mut dma_addr = 0x2666_0000;

	let mut dev = {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				for (i, b) in h.base_address.iter().enumerate() {
					println!("{}: {:x}", i, b.get());
				}

				let mut map_addr = 0x2000_0000 as *mut kernel::Page;

				let get_phys_addr = |addr| {
					let addr = NonNull::new(addr as *mut _).unwrap();
					syscall::physical_address(addr).unwrap()
				};
				let map_bar = |bar: u8| {
					let addr = map_addr.cast();
					syscall::map_object(handle, NonNull::new(addr), (bar + 1).into(), usize::MAX)
						.unwrap();
					map_addr = map_addr.wrapping_add(16);
					NonNull::new(addr as *mut _).unwrap()
				};
				let dma_alloc = |size| {
					println!("dma: {:#x}", dma_addr);
					let d = core::ptr::NonNull::new(dma_addr as *mut _).unwrap();
					let res = syscall::alloc_dma(Some(d), size).unwrap();
					dma_addr += res;
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), a))
				};
				let d =
					virtio_block::BlockDevice::new(h, get_phys_addr, map_bar, dma_alloc).unwrap();

				println!("pci status: {:#x}", h.status());

				d
			}
			_ => unreachable!(),
		}
	};

	let mut sectors = virtio_block::Sector::default();
	for (w, r) in sectors.0.iter_mut().zip(b"Greetings, fellow developers!") {
		*w = *r;
	}

	println!("writing the stuff...");
	dev.write(&sectors, 0, || ()).unwrap();
	println!("done writing the stuff");

	// Register new table of Streaming type
	let tbl = syscall::create_table("virtio-blk", syscall::TableType::Streaming).unwrap();

	// Register a new object
	// TODO

	let mut buf = [0; 1024];

	loop {
		// Wait for events from the table
		println!("ermaghed");

		let job = syscall::take_table_job(tbl, &mut buf, std::time::Duration::MAX).unwrap();

		println!("job: {:#?}", &job);

		match job.ty {
			syscall::Job::OPEN => (),
			syscall::Job::WRITE => {
				let data = &buf[..job.operation_size as usize];
				println!("write: {:?}", core::str::from_utf8(data));
			}
			_ => todo!(),
		}

		syscall::finish_table_job(tbl, job).unwrap();

		// Log events
		//while let Some(cmd) = cmds.pop_command() {
		/*
		{
			println!("[stream-table] {:?}", "open");
			Response::open(
				&cmd,
				NonNull::new(wr.get()).unwrap().cast(),
				12,
				NonNull::new(rd.get()).unwrap().cast(),
				12,
			)
		}
		{
			println!("[stream-table] {:?}", count);

			let wr: &[u8] = unsafe {
				core::slice::from_raw_parts(wr.get().cast(), 4096)
			};
			println!("{:#x?}", syscall::physical_address(NonNull::from(wr).cast()));
			println!("wr_i {}", wr_i % wr.len());
			for i in wr_i .. wr_i + count {
				println!("  > {:?}", char::try_from(wr[i % wr.len()]).unwrap());
			}

			wr_i += count;
			Response::write(
				&cmd,
				count,
			)
		}
		last_cmd = Some(cmd);
		i += 1;
		c.unwrap();mds.push_response(rsp);
		*/
		//}

		// Mark events as handled
	}
}
