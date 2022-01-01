#![no_std]
#![no_main]
#![feature(naked_functions)]

mod dev;

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::NonNull;
use core::time::Duration;
use kernel::{syscall, syslog};

#[export_name = "main"]
fn main() {
	syslog!("Hello, internet!");

	// Find virtio-net-pci device
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

	// Reserve & initialize device
	let handle = syscall::open_object(tbl, dev).unwrap();

	let pci_config = NonNull::new(0x1000_0000 as *mut _);
	let pci_config = syscall::map_object(handle, pci_config, 0, usize::MAX).unwrap();

	syslog!("handle: {:?}", handle);

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let dev = pci.get(0, 0, 0).unwrap();
	// FIXME figure out why InterfaceBuilder causes a 'static lifetime requirement
	let dev = unsafe { core::mem::transmute::<&_, &_>(&dev) };

	let mut dma_addr = 0x2666_0000;

	let (dev, addr) = {
		match dev {
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

	// Wrap the device for use with smoltcp
	use smoltcp::{iface, phy, socket, time, wire};
	let dev = dev::Dev::new(dev);
	let dev = phy::Tracer::new(dev, |t, p| syslog!("[{}] {}", t, p));
	let mut ip_addrs = [wire::IpCidr::new(wire::Ipv4Address::UNSPECIFIED.into(), 0)];
	let mut sockets = [iface::SocketStorage::EMPTY; 8];
	let mut neighbors = [None; 8];
	let mut routes = [None; 8];
	syslog!("{:?}", &addr);
	let mut iface = iface::InterfaceBuilder::new(dev, &mut sockets[..])
		.ip_addrs(&mut ip_addrs[..])
		.hardware_addr(wire::EthernetAddress(*addr.as_ref()).into())
		.neighbor_cache(iface::NeighborCache::new(&mut neighbors[..]))
		.routes(iface::Routes::new(&mut routes[..]))
		.finalize();

	// Get an IP address using DHCP
	let dhcp = iface.add_socket(socket::Dhcpv4Socket::new());

	// Create a TCP listener
	let mut rx @ mut tx = [0; 2048];
	let rx = socket::TcpSocketBuffer::new(&mut rx[..]);
	let tx = socket::TcpSocketBuffer::new(&mut tx[..]);
	let tcp = iface.add_socket(socket::TcpSocket::new(rx, tx));

	let mut t = time::Instant::from_secs(0);
	loop {
		iface.poll(t).unwrap();
		syslog!("ip addrs: {:?}", iface.ip_addrs());
		syslog!("routes  : {:?}", iface.routes());

		let dhcp = iface.get_socket::<socket::Dhcpv4Socket>(dhcp);
		if let Some(s) = dhcp.poll() {
			if let socket::Dhcpv4Event::Configured(s) = s {
				iface.update_ip_addrs(|i| i[0] = s.address.into());
				if let Some(r) = s.router {
					iface.routes_mut().add_default_ipv4_route(r).unwrap();
				}
			}
			continue;
		}

		let tcp = iface.get_socket::<socket::TcpSocket>(tcp);

		syslog!("tcp state: {}", tcp.state());
		syslog!("tcp keepalive: {:?}", tcp.keep_alive());
		if !tcp.is_open() {
			syslog!("open tcp");
			tcp.listen(333).unwrap();
		}
		if tcp.can_send() {
			syslog!("send tcp");
			use core::fmt::Write;
			write!(tcp, "Greetings, alien!").unwrap();
			syslog!("close tcp");
			tcp.close();
		}
		syslog!("rcv: {:?}", tcp.recv(|b| (b.len(), b.len())));

		syscall::sleep(Duration::from_secs(1));
		t += time::Duration::from_secs(1);
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
unsafe extern "C" fn start() -> ! {
	asm!(
		"
		lea		rsp, [rip + __stack + 16 * 0x1000]
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
static mut STACK: [P; 16] = [P([0; 4096]); 16];
