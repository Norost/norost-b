use {
	crate::{io, AsyncObject},
	alloc::format,
};

pub use no_std_net::*;

pub struct TcpListener(AsyncObject);

impl TcpListener {
	pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
		let root = rt::io::net_root().expect("no net root");
		let mut last_err = io::Error::InvalidData;
		for a in addr
			.to_socket_addrs()
			.unwrap_or_else(|_| todo!("convert error"))
		{
			let a = into_ip6(a);
			let path = format!("{}/tcp/listen/{}", a.ip(), a.port());
			match root.create(path.as_bytes()) {
				Ok(o) => return Ok(Self(o.into())),
				Err(e) => last_err = e,
			}
		}
		Err(last_err)
	}

	pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
		let (res, _) = self.0.open(b"accept").await;
		res.map(|obj| {
			/* FIXME sockaddr */
			(
				TcpStream(obj),
				SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)),
			)
		})
	}
}

pub struct TcpStream(AsyncObject);

impl_wrap!(TcpStream read);
impl_wrap!(TcpStream write);

impl TcpStream {}

fn into_ip6(addr: SocketAddr) -> SocketAddrV6 {
	match addr {
		SocketAddr::V4(addr) => {
			let [a, b, c, d] = addr.ip().octets();
			SocketAddrV6::new(
				Ipv6Addr::new(
					0,
					0,
					0,
					0,
					0,
					0xffff,
					u16::from(a) << 8 | u16::from(b),
					u16::from(c) << 8 | u16::from(d),
				),
				addr.port(),
				0,
				0,
			)
		}
		SocketAddr::V6(a) => a,
	}
}
