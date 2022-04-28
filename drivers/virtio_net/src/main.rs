#![feature(if_let_guard)]
#![feature(norostb)]

mod dev;

use core::ptr::NonNull;
use core::time::Duration;
use norostb_kernel::{io::Queue, syscall};
use norostb_rt::{
	self as rt,
	io::{Job, Request},
};
use smoltcp::socket::TcpState;
use std::fs;
use std::os::norostb::prelude::*;
use std::str::FromStr;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let table_name = std::env::args()
		.skip(1)
		.next()
		.ok_or("expected table name")?;

	let dev_handle = {
		let dev = fs::read_dir("pci/vendor-id:1af4&device-id:1000")
			.unwrap()
			.next()
			.unwrap()
			.unwrap();
		fs::File::open(dev.path()).unwrap().into_handle()
	};

	let pci_config = syscall::map_object(dev_handle, None, 0, usize::MAX).unwrap();

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let dev = pci.get(0, 0, 0).unwrap();
	// FIXME figure out why InterfaceBuilder causes a 'static lifetime requirement
	let dev = unsafe { core::mem::transmute::<&_, &_>(&dev) };

	let (dev, addr) = {
		match dev {
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
				unsafe { virtio_net::Device::new(h, map_bar, dma_alloc).unwrap() }
			}
			_ => unreachable!(),
		}
	};

	// Wrap the device for use with smoltcp
	use smoltcp::{iface, socket, time, wire};
	let dev = dev::Dev::new(dev);
	let mut ip_addrs = [wire::IpCidr::new(wire::Ipv4Address::UNSPECIFIED.into(), 0)];
	let mut sockets = [iface::SocketStorage::EMPTY; 8];
	let mut neighbors = [None; 8];
	let mut routes = [None; 8];
	let mut iface = iface::InterfaceBuilder::new(dev, &mut sockets[..])
		.ip_addrs(&mut ip_addrs[..])
		.hardware_addr(wire::EthernetAddress(*addr.as_ref()).into())
		.neighbor_cache(iface::NeighborCache::new(&mut neighbors[..]))
		.routes(iface::Routes::new(&mut routes[..]))
		.finalize();

	// Get an IP address using DHCP
	let dhcp = iface.add_socket(socket::Dhcpv4Socket::new());

	// Register new table of Streaming type
	let tbl = rt::io::base_object().create(table_name.as_bytes()).unwrap();

	#[derive(Clone, Copy)]
	enum Protocol {
		Udp,
		Tcp,
	}

	let mut t = time::Instant::from_secs(0);
	let mut buf = [0; 2048];
	let mut objects = Vec::new();

	let mut connecting_tcp_sockets = Vec::new();

	let mut job = Job::default();
	job.buffer = NonNull::new(buf.as_mut_ptr());
	job.buffer_size = buf.len().try_into().unwrap();

	// Use a separate queue we busypoll for jobs, as std::os::norostb::take_job blocks forever.
	let job_queue = syscall::create_io_queue(None, 0, 0).unwrap();
	let mut job_queue = Queue {
		base: job_queue.cast(),
		requests_mask: 0,
		responses_mask: 0,
	};

	unsafe {
		job_queue
			.enqueue_request(Request::take_job(0, tbl.as_raw(), &mut job))
			.unwrap();
	}

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
					let std_job = Job {
						ty: Job::CREATE,
						job_id,
						flags: [0; 3],
						handle: (objects.len() - 1).try_into().unwrap(),
						buffer: None,
						buffer_size: 0,
						operation_size: 0,
						from_anchor: 0,
						from_offset: 0,
					};
					tbl.finish_job(&std_job).unwrap();
					unsafe {
						job_queue
							.enqueue_request(Request::take_job(0, tbl.as_raw(), &mut job))
							.unwrap();
					}
				}
				s => todo!("{:?}", s),
			}
		}

		syscall::process_io_queue(Some(job_queue.base.cast())).unwrap();

		if let Ok(_) = unsafe { job_queue.dequeue_response() } {
			match job.ty {
				Job::CREATE => {
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

					job.handle = (objects.len() - 1).try_into().unwrap();
				}
				Job::READ => {
					let (sock, _addr, prot) = objects[job.handle as usize];
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
							job.operation_size = job.buffer_size;
						}
					}
				}
				Job::WRITE => {
					let (sock, addr, prot) = objects[job.handle as usize];
					match prot {
						Protocol::Udp => {
							let sock = iface.get_socket::<socket::UdpSocket>(sock);
							let data = &buf[..job.operation_size as usize];
							sock.send_slice(data, addr).unwrap();
						}
						Protocol::Tcp => {
							let sock = iface.get_socket::<socket::TcpSocket>(sock);
							let data = &buf[..job.operation_size as usize];
							sock.send_slice(data).unwrap();
						}
					}
				}
				Job::CLOSE => {
					todo!();
				}
				t => todo!("job type {}", t),
			}

			tbl.finish_job(&job).unwrap();
			unsafe {
				job_queue
					.enqueue_request(Request::take_job(0, tbl.as_raw(), &mut job))
					.unwrap();
			}
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
