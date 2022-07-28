use alloc::vec::Vec;
use smoltcp::{
	iface::{Interface, SocketHandle},
	phy::Device,
	socket::{TcpSocket, TcpSocketBuffer, TcpState},
	wire::IpEndpoint,
};

pub struct TcpListener<const PENDING_MAX: usize>
where
	[(); PENDING_MAX]: Sized,
{
	pending: [SocketHandle; PENDING_MAX],
	source: IpEndpoint,
}

impl<const PENDING_MAX: usize> TcpListener<PENDING_MAX>
where
	[SocketHandle; PENDING_MAX]: Sized + Default,
{
	pub fn new(
		iface: &mut Interface<impl for<'d> Device<'d>>,
		source: impl Into<IpEndpoint>,
	) -> Self {
		let source = source.into();
		let mut pending: [SocketHandle; PENDING_MAX] = Default::default();
		pending
			.iter_mut()
			.for_each(|p| *p = new_socket(iface, |s| s.listen(source).unwrap()));
		Self { pending, source }
	}

	pub fn accept(
		&mut self,
		iface: &mut Interface<impl for<'d> Device<'d>>,
	) -> Option<TcpConnection> {
		for p in self.pending.iter_mut() {
			let sock = iface.get_socket::<TcpSocket>(*p);
			if sock.is_active() {
				let handle = *p;
				*p = new_socket(iface, |s| s.listen(self.source).unwrap());
				return Some(TcpConnection { handle });
			}
		}
		None
	}
}

pub struct TcpConnection {
	handle: SocketHandle,
}

impl TcpConnection {
	pub fn new(
		iface: &mut Interface<impl for<'d> Device<'d>>,
		source: impl Into<IpEndpoint>,
		destination: impl Into<IpEndpoint>,
	) -> Self {
		let handle = new_socket(iface, |_| ());
		let (sock, cx) = iface.get_socket_and_context::<TcpSocket>(handle);
		sock.connect(cx, destination, source).unwrap();
		Self { handle }
	}

	pub fn ready(&self, iface: &mut Interface<impl for<'d> Device<'d>>) -> bool {
		let sock = iface.get_socket::<TcpSocket>(self.handle);
		sock.may_send() || sock.may_recv()
	}

	pub fn active(&self, iface: &mut Interface<impl for<'d> Device<'d>>) -> bool {
		let sock = iface.get_socket::<TcpSocket>(self.handle);
		sock.is_active()
	}

	pub fn read(
		&mut self,
		data: &mut [u8],
		iface: &mut Interface<impl for<'d> Device<'d>>,
	) -> smoltcp::Result<usize> {
		iface.get_socket::<TcpSocket>(self.handle).recv_slice(data)
	}

	pub fn write(
		&mut self,
		data: &[u8],
		iface: &mut Interface<impl for<'d> Device<'d>>,
	) -> smoltcp::Result<usize> {
		iface.get_socket::<TcpSocket>(self.handle).send_slice(data)
	}

	pub fn write_all(
		&mut self,
		data: &[u8],
		iface: &mut Interface<impl for<'d> Device<'d>>,
	) -> Option<smoltcp::Result<()>> {
		let s = iface.get_socket::<TcpSocket>(self.handle);
		(s.send_capacity() - s.send_queue() >= data.len())
			.then(|| s.send_slice(data).map(|l| debug_assert_eq!(l, data.len())))
	}

	pub fn close(&mut self, iface: &mut Interface<impl for<'d> Device<'d>>) {
		iface.get_socket::<TcpSocket>(self.handle).close();
	}

	pub fn remove(&mut self, iface: &mut Interface<impl for<'d> Device<'d>>) -> bool {
		let sock = iface.get_socket::<TcpSocket>(self.handle);
		let remove = sock.state() == TcpState::Closed;
		if remove {
			iface.remove_socket(self.handle);
		}
		remove
	}
}

fn new_socket(
	iface: &mut Interface<impl for<'d> Device<'d>>,
	f: impl FnOnce(&mut TcpSocket),
) -> SocketHandle {
	let rx = TcpSocketBuffer::new(Vec::from([0; 4096]));
	let tx = TcpSocketBuffer::new(Vec::from([0; 4096]));
	let mut sock = TcpSocket::new(rx, tx);
	f(&mut sock);
	iface.add_socket(sock)
}
