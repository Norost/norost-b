#![feature(if_let_guard)]
#![feature(never_type)]
#![feature(norostb)]
#![feature(type_alias_impl_trait)]

mod dev;
mod tcp;
mod udp;

use core::{future::Future, pin::Pin, str, task::Poll, time::Duration};
use driver_utils::io::queue::stream::Job;
use futures::stream::{FuturesUnordered, StreamExt};
use nora_io_queue_rt::{Pow2Size, Queue};
use norostb_kernel::syscall;
use norostb_rt::{self as rt, Error};
use smoltcp::wire;
use std::os::norostb::prelude::*;
use std::str::FromStr;
use std::{fs, io::Write};
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
		let s = b" 1af4:1000";
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
	let mut sockets = Vec::new();
	sockets.resize_with(1024, || iface::SocketStorage::EMPTY);
	let mut neighbors = [None; 8];
	let mut routes = [None; 8];
	let mut iface = iface::InterfaceBuilder::new(dev, sockets)
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

	// Use a separate queue we busypoll for jobs, as std::os::norostb::take_job blocks forever.
	let job_queue = Queue::new(Pow2Size::P6, Pow2Size::P6).unwrap();

	let new_job = |v| job_queue.submit_read(tbl.as_raw(), v, 2048).unwrap();
	let mut jobs = (0..Pow2Size::P6.size())
		.map(|_| new_job(Vec::new()))
		.collect::<FuturesUnordered<_>>();

	let mut alloc_port = 50_000u16;
	let mut alloc_port = || {
		alloc_port = alloc_port.wrapping_add(1).max(50_000);
		alloc_port
	};

	enum Query {
		Root(QueryRoot),
		SourceAddr(wire::Ipv6Address, Protocol),
	}

	enum QueryRoot {
		Default,
		Global,
		IpAddr(usize),
	}

	enum Object {
		Socket(Socket),
		Query(Option<Query>),
	}
	let mut objects = driver_utils::Arena::new();
	let mut connecting_tcp_sockets = Vec::<(TcpConnection, _, _)>::new();
	let mut accepted_tcp_sockets = Vec::<(TcpConnection, _, _)>::new();
	let mut accepting_tcp_sockets = Vec::new();
	let mut closing_tcp_sockets = Vec::<TcpConnection>::new();

	let mut i = 0u64;
	loop {
		if i % (512 * 60) == 0 {
			dbg!(connecting_tcp_sockets.len());
			dbg!(&job_queue);
		}
		i += 1;
		// Advance TCP connection state.
		for i in (0..connecting_tcp_sockets.len()).rev() {
			let (sock, _, _) = &connecting_tcp_sockets[i];
			if sock.ready(&mut iface) {
				let (sock, job_id, buf) = connecting_tcp_sockets.swap_remove(i);
				let handle = objects.insert(Object::Socket(Socket::TcpConnection(sock)));
				let buf = Job::reply_create_clear(buf, job_id, handle).unwrap();
				tbl.write(&buf).unwrap();
				jobs.push(new_job(buf));
			} else if !sock.active(&mut iface) {
				todo!()
			}
		}
		for i in (0..accepted_tcp_sockets.len()).rev() {
			let (sock, _, _) = &accepted_tcp_sockets[i];
			if sock.ready(&mut iface) {
				let (sock, job_id, buf) = accepted_tcp_sockets.swap_remove(i);
				let handle = objects.insert(Object::Socket(Socket::TcpConnection(sock)));
				let buf = Job::reply_create_clear(buf, job_id, handle).unwrap();
				tbl.write(&buf).unwrap();
				jobs.push(new_job(buf));
			} else if !sock.active(&mut iface) {
				// Try again
				todo!()
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
			let (handle, _, _) = &accepting_tcp_sockets[i];
			let c = match &mut objects[*handle] {
				Object::Socket(Socket::TcpListener(l)) => l.accept(&mut iface),
				_ => unreachable!(),
			};
			if let Some(sock) = c {
				let (_, job_id, buf) = accepting_tcp_sockets.swap_remove(i);
				accepted_tcp_sockets.push((sock, job_id, buf));
			}
		}

		job_queue.poll();
		job_queue.process();

		let w = driver_utils::task::waker::dummy();
		let mut cx = core::task::Context::from_waker(&w);

		let mut next_job = jobs.select_next_some();
		if let Poll::Ready(buf) = Pin::new(&mut next_job).poll(&mut cx) {
			let buf = buf.unwrap();
			let reply = match Job::deserialize(&buf).unwrap() {
				Job::Open {
					handle,
					job_id,
					path,
				} => {
					let path = str::from_utf8(path).unwrap();
					if path == "" || path.bytes().last() == Some(b'/') {
						// Query
						assert_eq!(handle, driver_utils::Handle::MAX, "TODO");
						let mut path = path.split('/');
						let query = match (path.next().unwrap(), path.next(), path.next()) {
							("", None, _) => Query::Root(QueryRoot::Default),
							("default", None, _) | ("default", Some(""), None) => {
								let addr = into_ip6(iface.ip_addrs()[0].address());
								Query::SourceAddr(addr, Protocol::Tcp)
							},
							(addr, None, _) | (addr, Some(""), None) if let Ok(addr) = wire::IpAddress::from_str(addr) => todo!(),
							path => todo!("{:?}", path),
						};
						let handle = objects.insert(Object::Query(Some(query)));
						Job::reply_open_clear(buf, job_id, handle).unwrap()
					} else {
						// Open
						assert_ne!(handle, driver_utils::Handle::MAX, "TODO");
						match &mut objects[handle] {
							Object::Socket(Socket::TcpListener(_)) => match path {
								"accept" => {
									accepting_tcp_sockets.push((handle, job_id, buf));
									continue;
								}
								_ => todo!(),
							},
							Object::Socket(Socket::TcpConnection(_)) => todo!(),
							Object::Socket(Socket::Udp(_)) => todo!(),
							Object::Query(_) => todo!(),
						}
					}
				}
				Job::Create {
					handle,
					job_id,
					path,
				} => {
					assert_eq!(handle, driver_utils::Handle::MAX, "TODO");
					let s = str::from_utf8(path).unwrap();
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
					let handle = objects.insert(Object::Socket(match parts.next().unwrap() {
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
										job_id,
										buf,
									));
									continue;
								}
								"active" => todo!(),
								_ => todo!(),
							}
						}
						"udp" => Socket::Udp(UdpSocket::new(&mut iface)),
						_ => todo!(),
					}));

					assert!(parts.next().is_none());

					Job::reply_create_clear(buf, job_id, handle).unwrap()
				}
				Job::Read {
					peek,
					handle,
					job_id,
					length,
				} => {
					let len = (length as usize).min(2000);
					match &mut objects[handle] {
						Object::Socket(Socket::TcpListener(_)) => todo!(),
						Object::Socket(Socket::TcpConnection(sock)) => {
							Job::reply_read_clear(buf, job_id, peek, |data| {
								let og_len = data.len();
								data.resize(og_len + len, 0);
								let r = if peek {
									sock.peek(&mut data[og_len..], &mut iface)
								} else {
									sock.read(&mut data[og_len..], &mut iface)
								};
								match r {
									Ok(l) => Ok(data.resize(og_len + l, 0)),
									Err(smoltcp::Error::Illegal)
									| Err(smoltcp::Error::Finished) => Err(()),
									Err(e) => todo!("handle {:?}", e),
								}
							})
							.or_else(|(buf, ())| {
								Job::reply_error_clear(buf, job_id, Error::Unknown)
							})
							.unwrap()
						}
						Object::Socket(Socket::Udp(_sock)) => {
							todo!("udp remote address")
						}
						Object::Query(q) => match q {
							Some(Query::Root(q @ QueryRoot::Default)) => {
								if !peek {
									*q = QueryRoot::Global;
								}
								Job::reply_read_clear(buf, job_id, peek, |v| {
									Ok(v.extend(b"default"))
								})
								.unwrap()
							}
							Some(Query::Root(q @ QueryRoot::Global)) => {
								if !peek {
									*q = QueryRoot::IpAddr(0);
								}
								Job::reply_read_clear(buf, job_id, peek, |v| Ok(v.extend(b"::")))
									.unwrap()
							}
							Some(Query::Root(QueryRoot::IpAddr(i))) => {
								let ip = into_ip6(iface.ip_addrs()[*i].address());
								if !peek {
									*i += 1;
									if *i >= iface.ip_addrs().len() {
										*q = None;
									}
								}
								Job::reply_read_clear(buf, job_id, peek, |v| {
									write!(v, "{}", ip).map_err(|_| ())
								})
								.unwrap()
							}
							Some(Query::SourceAddr(addr, p @ Protocol::Tcp)) => {
								if !peek {
									*p = Protocol::Udp;
								}
								Job::reply_read_clear(buf, job_id, peek, |v| {
									write!(v, "{}/tcp", addr).map_err(|_| ())
								})
								.unwrap()
							}
							Some(Query::SourceAddr(addr, Protocol::Udp)) => {
								let r = Job::reply_read_clear(buf, job_id, peek, |v| {
									write!(v, "{}/udp", addr).map_err(|_| ())
								})
								.unwrap();
								if !peek {
									*q = None;
								}
								r
							}
							None => Job::reply_read_clear(buf, job_id, peek, |_| Ok(())).unwrap(),
						},
					}
				}
				Job::Write {
					handle,
					job_id,
					data,
				} => match &mut objects[handle] {
					Object::Socket(Socket::TcpListener(_)) => todo!(),
					Object::Socket(Socket::TcpConnection(sock)) => {
						match sock.write(data, &mut iface) {
							Ok(l) => Job::reply_write_clear(buf, job_id, u64::try_from(l).unwrap())
								.unwrap(),
							Err(smoltcp::Error::Illegal) => {
								Job::reply_error_clear(buf, job_id, Error::Unknown).unwrap()
							}
							Err(e) => todo!("handle {:?}", e),
						}
					}
					Object::Socket(Socket::Udp(_sock)) => {
						todo!("udp remote address")
					}
					Object::Query(_) => todo!(),
				},
				Job::Close { handle } => {
					match objects.remove(handle).unwrap() {
						Object::Socket(Socket::TcpListener(_)) => todo!(),
						Object::Socket(Socket::TcpConnection(mut sock)) => {
							sock.close(&mut iface);
							closing_tcp_sockets.push(sock);
						}
						Object::Socket(Socket::Udp(sock)) => sock.close(&mut iface),
						Object::Query(_) => {}
					}
					jobs.push(new_job(buf));
					continue;
				}
				Job::Seek { job_id, .. } => {
					Job::reply_error_clear(buf, job_id, Error::InvalidOperation).unwrap()
				}
			};
			tbl.write(&reply).unwrap();
			jobs.push(new_job(reply));
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

		let d = Duration::from_millis(2);
		syscall::sleep(d);
		t += d.into();
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
