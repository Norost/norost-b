#![no_std]
#![feature(if_let_guard)]
#![feature(let_else)]
#![feature(never_type)]
#![feature(start)]
#![feature(type_alias_impl_trait)]

mod dev;
mod tcp;
mod udp;

extern crate alloc;

use alloc::{format, vec::Vec};

use async_std::{
	io::Read,
	net::Ipv6Addr,
	object::{AsyncObject, RefAsyncObject},
};
use core::{
	future::Future,
	pin::Pin,
	str::{self, FromStr},
	time::Duration,
};
use driver_utils::os::stream_table::{JobId, Request, Response, StreamTable};
use rt::Error;
use rt_default as _;
use smoltcp::wire;
use tcp::{TcpConnection, TcpListener};
use udp::UdpSocket;

enum Socket {
	TcpListener(TcpListener<5>),
	TcpConnection(TcpConnection),
	Udp(UdpSocket),
}

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main();
	0
}

fn main() {
	let file_root = rt::io::file_root().unwrap();
	let table_name = rt::args::args()
		.skip(1)
		.next()
		.expect("expected table name");

	let dev = rt::args::handles()
		.find(|(name, _)| name == b"pci")
		.expect("no 'pci' object")
		.1;
	let poll = AsyncObject::from_raw(dev.open(b"poll").unwrap().into_raw());

	let pci = dev.map_object(None, rt::RWX::R, 0, usize::MAX).unwrap();
	let pci = unsafe { pci::Pci::new(pci.0.cast(), 0, 0, &[]) };

	let pci = pci.get(0, 0, 0).unwrap();
	// FIXME figure out why InterfaceBuilder causes a 'static lifetime requirement
	let pci = unsafe { core::mem::transmute::<&_, &_>(&pci) };

	let (dev, addr) = {
		match pci {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					assert!(bar < 6);
					let mut s = *b"bar0";
					s[3] += bar;
					dev.open(&s)
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

				let msix = virtio_net::Msix {
					receive_queue: Some(0),
					transmit_queue: Some(1),
				};

				unsafe { virtio_net::Device::new(h, map_bar, dma_alloc, msix).unwrap() }
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

	let mut alloc_port = 50_000u16;
	let mut alloc_port = || {
		alloc_port = alloc_port.wrapping_add(1).max(50_000);
		alloc_port
	};

	let mut connecting_tcp_sockets = Vec::<(TcpConnection, _)>::new();
	let mut accepted_tcp_sockets = Vec::<(TcpConnection, _)>::new();
	let mut accepting_tcp_sockets = Vec::new();
	let mut closing_tcp_sockets = Vec::<TcpConnection>::new();

	let mut table = Table::new(table_name);
	let mut table_notify = RefAsyncObject::from(table.table.notifier()).read(());

	let mut poll_job = poll.read(());

	struct PendingWrite {
		handle: rt::Handle,
		job_id: JobId,
		data: alloc::boxed::Box<[u8]>,
	}
	struct PendingRead {
		handle: rt::Handle,
		job_id: JobId,
		len: u32,
	}
	// FIXME avoid closing before finishing.
	let mut pending_writes = Vec::<PendingWrite>::new();
	let mut pending_reads = Vec::<PendingRead>::new();

	let mut t;
	let mut buf = [0; 2048];
	loop {
		// Finish pending writes
		for i in (0..pending_writes.len()).rev() {
			let p = &mut pending_writes[i];
			match &mut table.objects[p.handle] {
				Object::Socket(Socket::TcpConnection(sock)) => {
					if let Some(r) = sock.write_all(&p.data, &mut iface) {
						r.unwrap();
						table.amount(p.job_id, p.data.len() as _);
						pending_writes.swap_remove(i);
					}
				}
				Object::Socket(Socket::Udp(_)) => todo!(),
				_ => unreachable!(),
			}
		}

		// Finish pending reads
		for i in (0..pending_reads.len()).rev() {
			let p = &mut pending_reads[i];
			match &mut table.objects[p.handle] {
				Object::Socket(Socket::TcpConnection(sock)) => {
					match sock.read(&mut buf[..p.len as _], &mut iface) {
						Ok(0) => {}
						Ok(l) => {
							table.data(p.job_id, &buf[..l]);
							pending_reads.swap_remove(i);
						}
						Err(smoltcp::Error::Illegal) | Err(smoltcp::Error::Finished) => {
							table.error(p.job_id, Error::Unknown);
							pending_reads.swap_remove(i);
						}
						Err(e) => todo!("{:?}", e),
					}
				}
				Object::Socket(Socket::Udp(_)) => todo!(),
				_ => unreachable!(),
			}
		}

		// Advance TCP connection state.
		for i in (0..connecting_tcp_sockets.len()).rev() {
			let (sock, _) = &connecting_tcp_sockets[i];
			if sock.ready(&mut iface) {
				let (sock, job_id) = connecting_tcp_sockets.swap_remove(i);
				table.insert(job_id, Object::Socket(Socket::TcpConnection(sock)));
			} else if !sock.active(&mut iface) {
				todo!()
			}
		}
		for i in (0..accepted_tcp_sockets.len()).rev() {
			let (sock, _) = &accepted_tcp_sockets[i];
			if sock.ready(&mut iface) {
				let (sock, job_id) = accepted_tcp_sockets.swap_remove(i);
				table.insert(job_id, Object::Socket(Socket::TcpConnection(sock)));
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
			let (handle, _) = &accepting_tcp_sockets[i];
			let c = match &mut table.objects[*handle] {
				Object::Socket(Socket::TcpListener(l)) => l.accept(&mut iface),
				_ => unreachable!(),
			};
			if let Some(sock) = c {
				let (_, job_id) = accepting_tcp_sockets.swap_remove(i);
				accepted_tcp_sockets.push((sock, job_id));
			}
		}

		let w = driver_utils::task::waker::dummy();
		let mut cx = core::task::Context::from_waker(&w);

		// Handle incoming requests
		loop {
			let Some((handle, job_id, req)) = table.table.dequeue() else { break };
			match req {
				v @ Request::Open { .. } => {
					let (path, _) = v.into_data().copy_into(&mut buf);
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
						table.insert(job_id, Object::Query(Some(query)));
					} else {
						// Open
						assert_ne!(handle, driver_utils::Handle::MAX, "TODO");
						match &mut table.objects[handle] {
							Object::Socket(Socket::TcpListener(_)) => match path {
								"accept" => {
									accepting_tcp_sockets.push((handle, job_id));
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
				v @ Request::Create { .. } => {
					if handle == rt::Handle::MAX {
						let (path, _) = v.into_data().copy_into(&mut buf);
						let path = str::from_utf8(path).unwrap();
						let mut parts = path.split('/');
						let source = match parts.next().unwrap() {
							"default" => iface.ip_addrs()[0].address(),
							source => {
								let source = Ipv6Addr::from_str(source).unwrap();
								if let Some(source) = source.to_ipv4() {
									wire::IpAddress::Ipv4(wire::Ipv4Address(source.octets()))
								} else {
									wire::IpAddress::Ipv6(wire::Ipv6Address(source.octets()))
								}
							}
						};
						table.insert(
							job_id,
							Object::Socket(match parts.next().unwrap() {
								// protocol
								"tcp" => {
									match parts.next().unwrap() {
										// type
										"listen" => {
											let port = parts.next().unwrap().parse().unwrap();
											let source = wire::IpEndpoint { addr: source, port };
											Socket::TcpListener(TcpListener::new(
												&mut iface, source,
											))
										}
										"connect" => {
											let dest = parts.next().unwrap();
											let dest = Ipv6Addr::from_str(dest).unwrap();
											let dest = dest.to_ipv4().map_or(
												wire::IpAddress::Ipv6(wire::Ipv6Address(
													dest.octets(),
												)),
												|dest| {
													wire::IpAddress::Ipv4(wire::Ipv4Address(
														dest.octets(),
													))
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
											));
											continue;
										}
										"active" => todo!(),
										_ => todo!(),
									}
								}
								"udp" => Socket::Udp(UdpSocket::new(&mut iface)),
								_ => todo!(),
							}),
						);

						assert!(parts.next().is_none());
					} else {
						drop(v);
						table.error(job_id, Error::InvalidOperation);
					}
				}
				v @ Request::Read { .. } => {
					let len = (v.into_amount() as usize).min(buf.len());
					match &mut table.objects[handle] {
						Object::Socket(Socket::TcpListener(_)) => {
							table.error(job_id, Error::InvalidOperation)
						}
						Object::Socket(Socket::TcpConnection(sock)) => {
							let r = sock.read(&mut buf[..len], &mut iface);
							match r {
								Ok(0) => pending_reads.push(PendingRead {
									handle,
									job_id,
									len: len.try_into().unwrap(),
								}),
								Ok(len) => table.data(job_id, &buf[..len]),
								Err(smoltcp::Error::Illegal) | Err(smoltcp::Error::Finished) => {
									table.error(job_id, Error::Unknown)
								}
								Err(e) => todo!("handle {:?}", e),
							}
						}
						Object::Socket(Socket::Udp(_sock)) => {
							todo!("udp remote address")
						}
						Object::Query(q) => match q {
							Some(Query::Root(q @ QueryRoot::Default)) => {
								*q = QueryRoot::Global;
								table.data(job_id, b"default")
							}
							Some(Query::Root(q @ QueryRoot::Global)) => {
								*q = QueryRoot::IpAddr(0);
								table.data(job_id, b"::")
							}
							Some(Query::Root(QueryRoot::IpAddr(i))) => {
								let ip = into_ip6(iface.ip_addrs()[*i].address());
								*i += 1;
								if *i >= iface.ip_addrs().len() {
									*q = None;
								}
								table.data(job_id, format!("{}", ip).as_bytes())
							}
							Some(Query::SourceAddr(addr, p @ Protocol::Tcp)) => {
								let addr = *addr;
								*p = Protocol::Udp;
								table.data(job_id, format!("{}/tcp", addr).as_bytes())
							}
							Some(Query::SourceAddr(addr, Protocol::Udp)) => {
								let addr = *addr;
								*q = None;
								table.data(job_id, format!("{}/udp", addr).as_bytes())
							}
							None => table.data(job_id, &[]),
						},
					}
				}
				v @ Request::Write { .. } => match &mut table.objects[handle] {
					Object::Socket(Socket::TcpListener(_)) => todo!(),
					Object::Socket(Socket::TcpConnection(sock)) => {
						let (data, _) = v.into_data().copy_into(&mut buf);
						match sock.write(data, &mut iface) {
							Ok(l) if l == 0 => {
								pending_writes.push(PendingWrite {
									handle,
									job_id,
									data: (&*data).into(),
								});
							}
							Ok(l) => table.amount(job_id, l),
							Err(smoltcp::Error::Illegal) => table.error(job_id, Error::Unknown),
							Err(e) => todo!("handle {:?}", e),
						}
					}
					Object::Socket(Socket::Udp(_sock)) => {
						todo!("udp remote address")
					}
					Object::Query(_) => todo!(),
				},
				Request::Close => {
					match table.objects.remove(handle).unwrap() {
						Object::Socket(Socket::TcpListener(_)) => todo!(),
						Object::Socket(Socket::TcpConnection(mut sock)) => {
							sock.close(&mut iface);
							closing_tcp_sockets.push(sock);
						}
						Object::Socket(Socket::Udp(sock)) => sock.close(&mut iface),
						Object::Query(_) => {}
					}
					continue;
				}
				v @ Request::Seek { .. } => {
					drop(v);
					table.error(job_id, Error::InvalidOperation);
				}
				Request::GetMeta { .. } => todo!(),
				Request::SetMeta { .. } => todo!(),
				Request::Destroy { .. } => todo!(),
				Request::Share { .. } => todo!(),
			}
		}
		table.flush();

		let dhcp = iface.get_socket::<socket::Dhcpv4Socket>(dhcp);
		if let Some(s) = dhcp.poll() {
			if let socket::Dhcpv4Event::Configured(s) = s {
				iface.update_ip_addrs(|i| i[0] = s.address.into());
				if let Some(r) = s.router {
					iface.routes_mut().add_default_ipv4_route(r).unwrap();
				}
			}
		}

		if Pin::new(&mut poll_job).poll(&mut cx).is_ready() {
			iface.device_mut().process();
			poll_job = poll.read(());
			continue;
		}
		if Pin::new(&mut table_notify).poll(&mut cx).is_ready() {
			table_notify = RefAsyncObject::from(table.table.notifier()).read(());
		}
		t = async_std::queue::poll();
		if let Some(delay) = iface.poll_delay(time::Instant::from_micros(t.as_micros() as i64)) {
			let delay = delay.into();
			if delay != Duration::ZERO {
				t = async_std::queue::wait(delay);
			}
		}

		iface
			.poll(time::Instant::from_micros(t.as_micros() as i64))
			.unwrap();
	}
}

#[derive(Clone, Copy)]
enum Protocol {
	Udp,
	Tcp,
}

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

struct Table {
	table: StreamTable,
	objects: driver_utils::Arena<Object>,
	dirty: bool,
}

impl Table {
	fn new(table_name: &[u8]) -> Self {
		let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 18 }).unwrap();
		let table = StreamTable::new(&buf, rt::io::Pow2Size(9), (1 << 12) - 1);
		rt::io::file_root()
			.unwrap()
			.create(table_name)
			.unwrap()
			.share(&table.public())
			.unwrap();
		Self {
			table,
			objects: Default::default(),
			dirty: false,
		}
	}

	fn insert(&mut self, job_id: JobId, object: Object) {
		let h = self.objects.insert(object);
		self.table.enqueue(job_id, Response::Handle(h));
		self.dirty = true;
	}

	fn error(&mut self, job_id: JobId, error: Error) {
		self.table.enqueue(job_id, Response::Error(error));
		self.dirty = true;
	}

	fn data(&mut self, job_id: JobId, data: &[u8]) {
		let b = self.table.alloc(data.len()).expect("out of buffers");
		b.copy_from(0, &data);
		self.table.enqueue(job_id, Response::Data(b));
		self.dirty = true;
	}

	fn amount(&mut self, job_id: JobId, amount: usize) {
		self.table
			.enqueue(job_id, Response::Amount(amount.try_into().unwrap()));
		self.dirty = true;
	}

	fn flush(&mut self) {
		if self.dirty {
			self.table.flush();
			self.dirty = false;
		}
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
