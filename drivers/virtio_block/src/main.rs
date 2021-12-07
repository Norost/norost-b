#![no_std]
#![no_main]
#![feature(asm, naked_functions)]

use kernel::syscall;
use kernel::syslog;

use core::mem;
use core::panic::PanicInfo;
use core::ptr::NonNull;
use core::time::Duration;

#[export_name = "main"]
extern "C" fn main() {
	syslog!("Hello, world! from Rust");

	let mut id = None;
	let mut dev = None;
	syslog!("iter tables");
	'found_dev: while let Some((i, inf)) = syscall::next_table(id) {
		syslog!("table: {:?} -> {:?}", i, core::str::from_utf8(inf.name()));
		if inf.name() == b"pci" {
			let tags: [syscall::Slice<u8>; 2] = [
				b"vendor-id:1af4".into(),
				b"device-id:1001".into(),
			];
			let h = syscall::query_table(i, None, &tags).unwrap();
			syslog!("{:?}", h);
			let mut buf = [0; 256];
			let mut obj = syscall::ObjectInfo::new(&mut buf);
			while let Ok(()) = syscall::query_next(h, &mut obj) {
				syslog!("{:#?}", &obj);
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

	use core::fmt::Write;
	syslog!("handle: {:?}", handle);

	let pci = unsafe {
		pci::Pci::new(
			pci_config.cast(),
			0,
			0,
			&[],
		)
	};

	let mut dma_addr = 0x2666_0000;

	let mut dev = unsafe {
		let h = pci.get(0, 0, 0).unwrap();
		match h {
			pci::Header::H0(h) => {
				for (i, b) in h.base_address.iter().enumerate() {
					syslog!("{}: {:x}", i, b.get());
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
				let dma_alloc = |size| unsafe {
					syslog!("dma: {:#x}", dma_addr);
					let d = core::ptr::NonNull::new(dma_addr as *mut _).unwrap();
					let res = syscall::alloc_dma(Some(d), size).unwrap();
					dma_addr += size;
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), a))
				};
				let d = virtio_block::BlockDevice::new(h, get_phys_addr, map_bar, dma_alloc).unwrap();

				syslog!("pci status: {:#x}", h.status());

				d
			}
			_ => unreachable!(),
		}
	};

	let mut sectors = virtio_block::Sector::default();
	for (w, r) in sectors.0.iter_mut().zip(b"Greetings, fellow developers!") {
		*w = *r;
	}

	let h = pci.get(0, 0, 0).unwrap();
	syslog!("writing the stuff...");
	dev.write(&sectors, 0, || ()).unwrap();
	syslog!("done writing the stuff");

	let (mut rd, mut wr) = (
		core::cell::UnsafeCell::new(P([0; 4096 / 8])),
		core::cell::UnsafeCell::new(P([0; 4096 / 8])),
	);

	// Register new table of Streaming type
	let tbl = syscall::create_table("virtio-blk", syscall::TableType::Streaming).unwrap();
	
	// Map the interface for the table
	let cmds = NonNull::new(0x6666_0000 as *mut _);
	let cmds = syscall::map_object(tbl, cmds, 0, 4096).unwrap();
	let cmds = unsafe {
		cmds.cast::<kernel::object_table::streaming::CommandQueue>().as_ref()
	};
	
	// Register a new object

	let mut wr_i = 0;

	let mut i = 0;
	let mut last_cmd = None;
	loop {
		// Wait for events from the table
		syslog!("ermaghed");
		syscall::sleep(Duration::MAX);

		// Log events
		syslog!("{:p}", cmds);
		syslog!("{:#x?}", syscall::physical_address(NonNull::from(cmds).cast()));
		while let Some(cmd) = cmds.pop_command() {
			use kernel::object_table::streaming::{Command, Response};
			let rsp = match cmd {
				Command::Open { .. } => {
					syslog!("[stream-table] {:?}", "open");
					Response::open(
						&cmd,
						NonNull::new(wr.get()).unwrap().cast(),
						12,
						NonNull::new(rd.get()).unwrap().cast(),
						12,
					)
				}
				Command::Write { count, .. } => {
					syslog!("[stream-table] {:?}", count);

					let wr: &[u8] = unsafe {
						core::slice::from_raw_parts(wr.get().cast(), 4096)
					};
					syslog!("{:#x?}", syscall::physical_address(NonNull::from(wr).cast()));
					syslog!("wr_i {}", wr_i % wr.len());
					for i in wr_i .. wr_i + count {
						syslog!("  > {:?}", char::try_from(wr[i % wr.len()]).unwrap());
					}

					wr_i += count;
					Response::write(
						&cmd,
						count,
					)
				}
				_ => todo!(),
			}.unwrap();
			last_cmd = Some(cmd);
			i += 1;
			cmds.push_response(rsp);
		}

		// Mark events as handled
	}
}

#[naked]
#[export_name = "_start"]
unsafe fn start() {
	asm!(
		"
		lea		rsp, [rip + __stack + 0x8000]

		#push	rax
		call	main

		mov		eax, 6
		xor		edi, edi
		syscall
	",
		options(noreturn)
	);
}

#[derive(Clone, Copy)]
#[repr(align(4096))]
struct P([u64; 512]);
#[export_name = "__stack"]
static mut STACK: [P; 0x8] = [P([0xdeadbeef; 4096 / 8]); 8];

static mut TEST: u8 = 8;
#[export_name = "test2__"]
static TEST2: u8 = 5;

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
	use core::fmt::Write;
	writeln!(syscall::SysLog::default(), "Panic! {:#?}", info);
	loop {}
}
