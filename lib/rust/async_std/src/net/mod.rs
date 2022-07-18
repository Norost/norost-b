use crate::{io, AsyncObject};
use core::{pin::Pin, task::Context};

pub use no_std_net::*;

pub struct TcpListener(AsyncObject);

impl TcpListener {
	pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
		todo!()
	}
}

pub struct TcpStream(AsyncObject);

impl_wrap!(TcpStream read);
impl_wrap!(TcpStream write);

impl TcpStream {}
