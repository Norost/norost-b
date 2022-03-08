#![feature(if_let_guard)]

mod dev;

use core::ptr::NonNull;
use core::time::Duration;
use norostb_kernel::{self as kernel, syscall};
use smoltcp::socket::TcpState;
use std::str::FromStr;

fn main() {
	println!("Hello, internet!");

	// Find virtio-net-pci device
	let mut id = None;
	let mut dev = None;
	println!("iter tables");
	'found_dev: while let Some((i, inf)) = syscall::next_table(id) {
		println!("table: {:?} -> {:?}", i, core::str::from_utf8(inf.name()));
		if inf.name() == b"pci" {
			let tags: [syscall::Slice<u8>; 2] =
				[b"vendor-id:1af4".into(), b"device-id:1000".into()];
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

	// Reserve & initialize device
	let handle = syscall::open(tbl, dev).unwrap();

	let pci_config = NonNull::new(0x1000_0000 as *mut _);
	let pci_config = syscall::map_object(handle, pci_config, 0, usize::MAX).unwrap();

	println!("handle: {:?}", handle);

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let dev = pci.get(0, 0, 0).unwrap();
	// FIXME figure out why InterfaceBuilder causes a 'static lifetime requirement
	let dev = unsafe { core::mem::transmute::<&_, &_>(&dev) };

	let mut dma_addr = 0x2666_0000;

	let (dev, addr) = {
		match dev {
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
					println!("  adr: {:p}", d);
					let res = syscall::alloc_dma(Some(d), size).unwrap();
					println!("  res: {} (>= {})", res, size);
					dma_addr += res;
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), a))
				};
				let d = virtio_net::Device::new(h, get_phys_addr, map_bar, dma_alloc).unwrap();

				println!("pci status: {:#x}", h.status());

				d
			}
			_ => unreachable!(),
		}
	};

	// Wrap the device for use with smoltcp
	use smoltcp::{iface, phy, socket, time, wire};
	let dev = dev::Dev::new(dev);
	//let dev = phy::Tracer::new(dev, |t, p| println!("[{}] {}", t, p));
	let mut ip_addrs = [wire::IpCidr::new(wire::Ipv4Address::UNSPECIFIED.into(), 0)];
	let mut sockets = [iface::SocketStorage::EMPTY; 8];
	let mut neighbors = [None; 8];
	let mut routes = [None; 8];
	println!("{:?}", &addr);
	let mut iface = iface::InterfaceBuilder::new(dev, &mut sockets[..])
		.ip_addrs(&mut ip_addrs[..])
		.hardware_addr(wire::EthernetAddress(*addr.as_ref()).into())
		.neighbor_cache(iface::NeighborCache::new(&mut neighbors[..]))
		.routes(iface::Routes::new(&mut routes[..]))
		.finalize();

	// Get an IP address using DHCP
	let dhcp = iface.add_socket(socket::Dhcpv4Socket::new());

	// Register new table of Streaming type
	let tbl = syscall::create_table("virtio-net", syscall::TableType::Streaming).unwrap();

	// Create a TCP listener
	let mut rx @ mut tx = [0; 2048];
	let rx = socket::TcpSocketBuffer::new(&mut rx[..]);
	let tx = socket::TcpSocketBuffer::new(&mut tx[..]);
	let tcp = iface.add_socket(socket::TcpSocket::new(rx, tx));

	#[derive(Clone, Copy)]
	enum Protocol {
		Udp,
		Tcp,
	}

	/*
	enum Socket {
		Udp {
			socket: socket::UdpSocket,
			address: IpEndpoint,
		},
		Tcp {
			socket: socket::TcpSocket,
			state: TcpState
		},
	}
	*/

	let mut t = time::Instant::from_secs(0);
	let mut buf = [0; 2048];
	let mut objects = Vec::new();

	let mut connecting_tcp_sockets = Vec::new();

	loop {
		// Advance TCP connection state.
		for i in (0..connecting_tcp_sockets.len()).rev() {
			let (sock_h, job_id) = connecting_tcp_sockets[i];
			let sock = iface.get_socket::<socket::TcpSocket>(sock_h);
			match sock.state() {
				TcpState::SynSent | TcpState::SynReceived => {}
				TcpState::Established => {
					connecting_tcp_sockets.remove(i);
					objects.push((
						sock_h,
						smoltcp::wire::IpEndpoint::UNSPECIFIED,
						Protocol::Tcp,
					));
					let job = syscall::Job {
						ty: syscall::Job::CREATE,
						job_id,
						flags: [0; 3],
						object_id: syscall::Id((objects.len() - 1).try_into().unwrap()),
						buffer: None,
						buffer_size: 0,
						operation_size: 0,
					};
					syscall::finish_table_job(tbl, job).unwrap();
				}
				s => todo!("{:?}", s),
			}
		}

		while let Ok(mut job) = syscall::take_table_job(tbl, &mut buf, Duration::new(0, 0)) {
			match job.ty {
				syscall::Job::CREATE => {
					let s = &buf[..job.operation_size as usize];

					let mut protocol = None;
					let mut port = None;
					let mut address = None;

					for tag in s.split(|c| *c == b'&') {
						let tag = std::str::from_utf8(tag).unwrap();
						match tag {
							"udp" => protocol = Some(Protocol::Udp),
							"tcp" => protocol = Some(Protocol::Tcp),
							s if let Ok(n) = u16::from_str_radix(s, 10) => port = Some(n),
							s if let Ok(a) = std::net::Ipv6Addr::from_str(s) => address = Some(a),
							_ => todo!(),
						}
					}

					use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv6Address};
					let addr = match address.unwrap() {
						a if let Some(a) = a.to_ipv4() => IpAddress::Ipv4(Ipv4Address(a.octets())),
						a => IpAddress::Ipv6(Ipv6Address(a.octets())),
					};
					let addr = IpEndpoint {
						addr,
						port: port.unwrap(),
					};

					match protocol.unwrap() {
						Protocol::Udp => {
							use smoltcp::storage::{PacketBuffer, PacketMetadata};
							let f = |n| (0..n).map(|_| PacketMetadata::EMPTY).collect::<Vec<_>>();
							let g = |n| (0..n).map(|_| 0u8).collect::<Vec<_>>();
							let rx = PacketBuffer::new(f(5), g(1024));
							let tx = PacketBuffer::new(f(5), g(1024));
							let mut sock = socket::UdpSocket::new(rx, tx);
							let local_addr = IpEndpoint {
								addr: IpAddress::Ipv4(Ipv4Address([10, 0, 2, 15])),
								port: 6666,
							};
							sock.bind(local_addr).unwrap();
							let sock = iface.add_socket(sock);
							objects.push((sock, addr, Protocol::Udp));
						}
						Protocol::Tcp => {
							use smoltcp::storage::RingBuffer;
							let g = |n| (0..n).map(|_| 0u8).collect::<Vec<_>>();
							let rx = RingBuffer::new(g(1024));
							let tx = RingBuffer::new(g(1024));
							let local_addr = IpEndpoint {
								addr: IpAddress::Ipv4(Ipv4Address([10, 0, 2, 15])),
								port: 6666,
							};
							let sock = socket::TcpSocket::new(rx, tx);
							let sock = iface.add_socket(sock);
							connecting_tcp_sockets.push((sock, job.job_id));
							let (sock, cx) =
								iface.get_socket_and_context::<socket::TcpSocket>(sock);
							sock.connect(cx, addr, local_addr).unwrap();
							continue;
						}
					}

					job.object_id = syscall::Id((objects.len() - 1).try_into().unwrap());
				}
				syscall::Job::READ => {
					let (sock, addr, prot) = objects[job.object_id.0 as usize];
					match prot {
						Protocol::Udp => {
							todo!("address");
							/*
							let sock = iface.get_socket::<socket::UdpSocket>(sock);
							let data = unsafe { job.data() };
							sock.recv_slice(data, addr).unwrap();
							*/
						}
						Protocol::Tcp => {
							let sock = iface.get_socket::<socket::TcpSocket>(sock);
							let len = (job.operation_size as usize).min(buf.len());
							if let Ok(len) = sock.recv_slice(&mut buf[..len]) {
								job.buffer = NonNull::new(buf.as_mut_ptr());
								job.buffer_size = len.try_into().unwrap();
							} else {
								job.buffer_size = 0;
							}
						}
					}
				}
				syscall::Job::WRITE => {
					let (sock, addr, prot) = objects[job.object_id.0 as usize];
					match prot {
						Protocol::Udp => {
							let sock = iface.get_socket::<socket::UdpSocket>(sock);
							let data = &buf[..job.operation_size as usize];
							sock.send_slice(data, addr).unwrap();
						}
						Protocol::Tcp => {
							let sock = iface.get_socket::<socket::TcpSocket>(sock);
							let data = &buf[..job.operation_size as usize];
							let e = sock.send_slice(data);
						}
					}
				}
				t => todo!("job type {}", t),
			}

			syscall::finish_table_job(tbl, job).unwrap();
		}

		iface.poll(t).unwrap();

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

		syscall::sleep(Duration::from_secs(1) / 10);
		t += time::Duration::from_secs(1) / 10;
	}
}
