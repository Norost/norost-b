#![no_std]
#![no_main]
#![feature(naked_functions)]

mod dev;

use core::arch::asm;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::ptr::NonNull;
use core::time::Duration;
use kernel::{syscall, syslog};

#[export_name = "main"]
fn main() {
	syslog!("Hello, internet!");

	let mut id = None;
	let mut dev = None;
	syslog!("iter tables");
	'found_dev: while let Some((i, inf)) = syscall::next_table(id) {
		syslog!("table: {:?} -> {:?}", i, core::str::from_utf8(inf.name()));
		if inf.name() == b"pci" {
			let tags: [syscall::Slice<u8>; 2] =
				[b"vendor-id:1af4".into(), b"device-id:1000".into()];
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

	syslog!("handle: {:?}", handle);

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let mut dma_addr = 0x2666_0000;

	let mut dev = {
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
				let dma_alloc = |size| {
					syslog!("dma: {:#x}", dma_addr);
					let d = core::ptr::NonNull::new(dma_addr as *mut _).unwrap();
					syslog!("  adr: {:p}", d);
					let res = syscall::alloc_dma(Some(d), size).unwrap();
					syslog!("  res: {} (>= {})", res, size);
					dma_addr += res;
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), a))
				};
				let d = virtio_net::Device::new(h, get_phys_addr, map_bar, dma_alloc).unwrap();

				syslog!("pci status: {:#x}", h.status());

				d
			}
			_ => unreachable!(),
		}
	};

	loop {
		let mut data = [0; 2048];
		if dev.receive(&mut data).unwrap() {
			use smoltcp::wire::*;
			syslog!("DATAAAAAAAAA");
			let fr = EthernetFrame::new_checked(&data[..]).unwrap();
			syslog!("  {}", fr);
			match fr.ethertype() {
				EthernetProtocol::Arp => {
					let pk = ArpPacket::new_checked(fr.payload()).unwrap();
					syslog!("  {:#}", pk);
				}
				_ => todo!(),
			}
		} else {
			syscall::sleep(Duration::from_millis(1000));
		}
	}
}

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
	syslog!("Panic! {:#?}", info);
	loop {
		syscall::sleep(Duration::MAX);
	}
}

#[naked]
#[export_name = "_start"]
unsafe extern "C" fn start() {
	asm!(
		"
		lea		rsp, [rip + __stack + 4 * 0x1000]
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
struct P([u8; 4096]);
#[export_name = "__stack"]
static mut STACK: [P; 4] = [P([0; 4096]); 4];
