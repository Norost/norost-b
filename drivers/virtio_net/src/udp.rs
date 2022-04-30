use smoltcp::{
	iface::{Interface, SocketHandle},
	phy::Device,
	socket::{self, UdpPacketMetadata, UdpSocketBuffer},
};

pub struct UdpSocket {
	handle: SocketHandle,
}

impl UdpSocket {
	pub fn new(iface: &mut Interface<impl for<'d> Device<'d>>) -> Self {
		let rx = UdpSocketBuffer::new(
			Vec::from([UdpPacketMetadata::EMPTY; 5]),
			Vec::from([0; 1024]),
		);
		let tx = UdpSocketBuffer::new(
			Vec::from([UdpPacketMetadata::EMPTY; 5]),
			Vec::from([0; 1024]),
		);
		let sock = socket::UdpSocket::new(rx, tx);
		let handle = iface.add_socket(sock);
		Self { handle }
	}
}

pub struct TcpConnection {
	handle: SocketHandle,
}
