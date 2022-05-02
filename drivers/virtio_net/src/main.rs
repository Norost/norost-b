#![feature(if_let_guard)]
#![feature(never_type)]
#![feature(norostb)]
#![feature(type_alias_impl_trait)]

mod dev;
mod tcp;
mod udp;

use core::ptr::NonNull;
use core::time::Duration;
use norostb_kernel::{io::Queue, syscall};
use norostb_rt::{
	self as rt,
	io::{Job, Request},
};
use smoltcp::{iface::SocketHandle, socket::TcpState, wire};
use std::fs;
use std::os::norostb::prelude::*;
use std::str::FromStr;
use tcp::{TcpConnection, TcpListener};
use udp::UdpSocket;

enum Socket {
	TcpListener(TcpListener<5>),
	TcpConnection(TcpConnection),
	Udp(UdpSocket),
}

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
	use smoltcp::{iface, socket, time};
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
	let tbl = rt::io::file_root()
		.unwrap()
		.create(table_name.as_bytes())
		.unwrap();

	#[derive(Clone, Copy)]
	enum Protocol {
		Udp,
		Tcp,
	}

	let mut t = time::Instant::from_secs(0);
	let mut buf = [0; 2048];

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

	let mut alloc_port = 50_000u16;
	let mut alloc_port = || {
		alloc_port = alloc_port.wrapping_add(1).max(50_000);
		alloc_port
	};

	enum Query {
		Root(QueryRoot),
		SourceAddr(wire::Ipv6Address, Protocol),
		DestAddr {
			source: wire::IpAddress,
			protocol: Protocol,
			dest: wire::IpAddress,
		},
		DestPort {
			source: wire::IpAddress,
			protocol: Protocol,
			dest: wire::IpAddress,
			port: u16,
		},
	}

	enum QueryRoot {
		Default,
		Global,
		IpAddr(usize),
	}

	let mut queries = driver_utils::Arena::new();
	let mut sockets = driver_utils::Arena::new();
	let mut connecting_tcp_sockets = Vec::<(TcpConnection, _)>::new();
	let mut accepting_tcp_sockets = Vec::new();
	let mut closing_tcp_sockets = Vec::<TcpConnection>::new();

	loop {
		// Advance TCP connection state.
		for i in (0..connecting_tcp_sockets.len()).rev() {
			let (sock, _) = &connecting_tcp_sockets[i];
			if sock.ready(&mut iface) {
				let (sock, job_id) = connecting_tcp_sockets.swap_remove(i);
				let handle = sockets.insert(Socket::TcpConnection(sock));
				let std_job = Job {
					ty: Job::CREATE,
					job_id,
					result: 0,
					handle,
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
		}

		// Remove closed TCP connections.
		for i in (0..closing_tcp_sockets.len()).rev() {
			let sock = &mut closing_tcp_sockets[i];
			if sock.remove(&mut iface) {
				closing_tcp_sockets.swap_remove(i);
			}
		}

		// Accept incoming TCP connections.
		for i in (0..accepting_tcp_sockets.len()).rev() {
			let (handle, _) = accepting_tcp_sockets[i];
			let c = match &mut sockets[handle] {
				Socket::TcpListener(l) => l.accept(&mut iface),
				_ => unreachable!(),
			};
			if let Some(sock) = c {
				let (_, job_id) = accepting_tcp_sockets.swap_remove(i);
				connecting_tcp_sockets.push((sock, job_id));
			}
		}

		syscall::process_io_queue(Some(job_queue.base.cast())).unwrap();

		if let Ok(_) = unsafe { job_queue.dequeue_response() } {
			match job.ty {
				Job::OPEN => {
					assert_ne!(job.handle, driver_utils::Handle::MAX, "TODO");
					let path = &buf[..job.operation_size as usize];
					let path = core::str::from_utf8(path).unwrap();
					match &mut sockets[job.handle] {
						Socket::TcpListener(_) => match path {
							"accept" => {
								accepting_tcp_sockets.push((job.handle, job.job_id));
								continue;
							}
							_ => todo!(),
						},
						Socket::TcpConnection(_) => todo!(),
						Socket::Udp(_) => todo!(),
					}
				}
				Job::CREATE => {
					assert_eq!(job.handle, driver_utils::Handle::MAX, "TODO");
					let s = &buf[..job.operation_size as usize];
					let s = core::str::from_utf8(s).unwrap();
					let mut parts = s.split('/');
					let source = match parts.next().unwrap() {
						"default" => iface.ip_addrs()[0].address(),
						source => {
							let source = std::net::Ipv6Addr::from_str(source).unwrap();
							if let Some(source) = source.to_ipv4() {
								wire::IpAddress::Ipv4(wire::Ipv4Address(source.octets()))
							} else {
								wire::IpAddress::Ipv6(wire::Ipv6Address(source.octets()))
							}
						}
					};
					job.handle = sockets.insert(match parts.next().unwrap() {
						// protocol
						"tcp" => {
							match parts.next().unwrap() {
								// type
								"listen" => {
									let port = parts.next().unwrap().parse().unwrap();
									let source = wire::IpEndpoint { addr: source, port };
									Socket::TcpListener(TcpListener::new(&mut iface, source))
								}
								"connect" => {
									let dest = parts.next().unwrap();
									let dest = std::net::Ipv6Addr::from_str(dest).unwrap();
									let dest = dest.to_ipv4().map_or(
										wire::IpAddress::Ipv6(wire::Ipv6Address(dest.octets())),
										|dest| {
											wire::IpAddress::Ipv4(wire::Ipv4Address(dest.octets()))
										},
									);
									let port = parts.next().unwrap().parse().unwrap();
									let source = wire::IpEndpoint {
										addr: source,
										port: alloc_port(),
									};
									let dest = wire::IpEndpoint { addr: dest, port };

									connecting_tcp_sockets.push((
										TcpConnection::new(&mut iface, source, dest),
										job.job_id,
									));
									continue;
								}
								"active" => todo!(),
								_ => todo!(),
							}
						}
						"udp" => Socket::Udp(UdpSocket::new(&mut iface)),
						_ => todo!(),
					});

					assert!(parts.next().is_none());
				}
				Job::READ => {
					let len = usize::try_from(job.operation_size).unwrap().min(buf.len());
					let data = &mut buf[..len];
					match &mut sockets[job.handle] {
						Socket::TcpListener(_) => todo!(),
						Socket::TcpConnection(sock) => {
							job.operation_size =
								sock.read(data, &mut iface).unwrap().try_into().unwrap();
							job.buffer = NonNull::new(data.as_mut_ptr());
						}
						Socket::Udp(sock) => {
							todo!("address")
						}
					}
				}
				Job::WRITE => {
					let data = &buf[..job.operation_size as usize];
					match &mut sockets[job.handle] {
						Socket::TcpListener(_) => todo!(),
						Socket::TcpConnection(sock) => {
							sock.write(data, &mut iface).unwrap();
						}
						Socket::Udp(sock) => {
							todo!("address")
						}
					}
				}
				Job::CLOSE => {
					match sockets.remove(job.handle).unwrap() {
						Socket::TcpListener(_) => todo!(),
						Socket::TcpConnection(mut sock) => {
							sock.close(&mut iface);
							closing_tcp_sockets.push(sock);
						}
						Socket::Udp(sock) => {
							todo!("address")
						}
					}
					unsafe {
						job_queue
							.enqueue_request(Request::take_job(0, tbl.as_raw(), &mut job))
							.unwrap();
					}
					continue;
				}
				Job::QUERY => {
					assert_eq!(job.handle, driver_utils::Handle::MAX, "TODO");
					let mut path = core::str::from_utf8(&buf[..job.operation_size as usize])
						.unwrap()
						.split('/');
					let query = match (path.next().unwrap(), path.next(), path.next()) {
						("", None, _) => Query::Root(QueryRoot::Default),
						("default", None, _) | ("default", Some(""), None) => {
							let addr = into_ip6(iface.ip_addrs()[0].address());
							Query::SourceAddr(addr, Protocol::Tcp)
						},
						(addr, None, _) | (addr, Some(""), None) if let Ok(addr) = wire::IpAddress::from_str(addr) => todo!(),
						path => todo!("{:?}", path),
					};
					job.handle = queries.insert(query);
				}
				Job::QUERY_NEXT => {
					use std::io::Write;
					match queries.get_mut(job.handle) {
						Some(Query::Root(q @ QueryRoot::Default)) => {
							let s = b"default";
							buf[..s.len()].copy_from_slice(s);
							job.operation_size = s.len().try_into().unwrap();
							*q = QueryRoot::Global;
						}
						Some(Query::Root(q @ QueryRoot::Global)) => {
							let s = b"::";
							buf[..s.len()].copy_from_slice(s);
							job.operation_size = s.len().try_into().unwrap();
							*q = QueryRoot::IpAddr(0);
						}
						Some(Query::Root(QueryRoot::IpAddr(i))) => {
							let mut b = &mut buf[..];
							write!(b, "{}", into_ip6(iface.ip_addrs()[*i].address())).unwrap();
							let l = b.len();
							job.operation_size = (buf.len() - l).try_into().unwrap();
							*i += 1;
							if *i >= iface.ip_addrs().len() {
								queries.remove(job.handle);
							}
						}
						Some(Query::SourceAddr(addr, p @ Protocol::Tcp)) => {
							let mut b = &mut buf[..];
							write!(b, "{}/tcp", addr).unwrap();
							let l = b.len();
							job.operation_size = (buf.len() - l).try_into().unwrap();
							*p = Protocol::Udp;
						}
						Some(Query::SourceAddr(addr, Protocol::Udp)) => {
							let mut b = &mut buf[..];
							write!(b, "{}/udp", addr).unwrap();
							let l = b.len();
							job.operation_size = (buf.len() - l).try_into().unwrap();
							queries.remove(job.handle);
						}
						Some(_) => todo!(),
						None => job.operation_size = 0,
					}
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

fn into_ip6(addr: wire::IpAddress) -> wire::Ipv6Address {
	match addr {
		wire::IpAddress::Ipv4(wire::Ipv4Address([a, b, c, d])) => wire::Ipv6Address::new(
			0,
			0,
			0,
			0,
			0,
			0xffff,
			u16::from(a) << 8 | u16::from(b),
			u16::from(c) << 8 | u16::from(d),
		),
		wire::IpAddress::Ipv6(a) => a,
		// Non-exhaustive cause *shrug*
		_ => todo!("unsupported address type"),
	}
}
