extern crate alloc;

use alloc::{boxed::Box, rc::Rc};
use async_std::{
	compat::{AsyncWrapR, AsyncWrapRW, AsyncWrapW},
	env,
	net::{Ipv4Addr, TcpListener, TcpStream},
	process,
};
use clap::Parser;
use core::{
	cell::{Cell, RefCell, RefMut},
	future::Future,
	ops::{Deref, DerefMut},
	pin::Pin,
	task::{Context, Poll, Waker},
};
use futures::{
	future::{FusedFuture, FutureExt},
	io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf},
	pin_mut, select,
	stream::{FuturesUnordered, StreamExt},
	stream_select,
};
use nora_ssh::{
	cipher,
	server::{IoSet, Server, ServerHandlers, SpawnType},
	Identifier,
};
use rand::{rngs::StdRng, CryptoRng, RngCore, SeedableRng};
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
struct Keys {
	ecdsa: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
	keys: Keys,
}

#[derive(Parser, Debug)]
struct Args {
	#[clap(value_parser)]
	config: String,
}

fn main() -> ! {
	let args = Args::parse();

	let config = std::fs::read(&args.config).unwrap();
	let config: Config = match toml::from_slice(&config) {
		Ok(p) => p,
		Err(e) => {
			eprintln!("{}", e);
			std::process::exit(1);
		}
	};

	let key = config.keys.ecdsa.expect("no keys");
	let key = std::fs::read(&key).unwrap();
	let key = std::str::from_utf8(&key).expect("invalid key");
	let key = key.trim();
	let key = base85::decode(key).expect("invalid key");
	let key = ecdsa::SigningKey::<p256::NistP256>::from_bytes(&key).expect("invalid key");

	async_std::task::block_on(async {
		let addr = (Ipv4Addr::UNSPECIFIED, 22);
		let listener = TcpListener::bind(addr).await.unwrap();
		let server = Server::new(
			Identifier::new(b"SSH-2.0-nora_ssh example").unwrap(),
			key,
			Handlers { listener },
		);

		server.start().await
	})
}

struct Handlers {
	listener: TcpListener,
}

struct User {
	name: Box<str>,
	shell: Option<Rc<process::Child>>,
}

#[async_trait::async_trait(?Send)]
impl ServerHandlers for Handlers {
	type Sign = p256::NistP256;
	type Crypt = cipher::ChaCha20Poly1305;
	type Read = ReadHalf<AsyncWrapRW<TcpStream>>;
	type Write = WriteHalf<AsyncWrapRW<TcpStream>>;
	type User = User;
	type Stdin = AsyncWrapW<process::ChildStdin>;
	type Stdout = AsyncWrapR<process::ChildStdout>;
	type Stderr = AsyncWrapR<process::ChildStderr>;
	type Rng = StdRng;

	async fn accept(&self) -> (Self::Read, Self::Write) {
		AsyncWrapRW::new(self.listener.accept().await.unwrap().0).split()
	}

	async fn authenticate<'a>(&self, data: &'a [u8]) -> Result<Self::User, ()> {
		Ok(User {
			name: "TODO".into(),
			shell: None,
		})
	}

	async fn spawn<'a>(
		&self,
		user: &'a mut Self::User,
		ty: SpawnType<'a>,
		data: &'a [u8],
	) -> Result<IoSet<Self::Stdin, Self::Stdout, Self::Stderr>, ()> {
		let wait = |child: Rc<process::Child>| async move {
			child.wait().await.unwrap().code().unwrap_or(0) as u32
		};
		match ty {
			SpawnType::Shell => {
				let shell = "drivers/minish";
				let shell = process::Command::new(shell)
					.await
					.stdin(process::Stdio::piped())
					.await
					.stdout(process::Stdio::piped())
					.await
					.stderr(process::Stdio::piped())
					.await
					.spawn()
					.await
					.unwrap();
				let mut shell = Rc::new(shell);
				let sh_mut = Rc::get_mut(&mut shell).unwrap();
				let io = IoSet {
					stdin: sh_mut.stdin.take().map(AsyncWrapW::new),
					stdout: sh_mut.stdout.take().map(AsyncWrapR::new),
					stderr: sh_mut.stderr.take().map(AsyncWrapR::new),
					wait: Box::pin(wait(shell.clone())),
				};
				user.shell = Some(shell);
				Ok(io)
			}
			SpawnType::Exec { command } => {
				let mut args = command
					.split(|c| c.is_ascii_whitespace())
					.filter(|s| !s.is_empty());
				let bin = args.next().unwrap();
				let bin = match bin {
					b"scp" => b"drivers/scp", // TODO do this properly with PATH env or whatever
					b => b,
				};
				let shell = process::Command::new(bin)
					.await
					.stdin(process::Stdio::piped())
					.await
					.stdout(process::Stdio::piped())
					.await
					.stderr(process::Stdio::piped())
					.await
					.args(args)
					.await
					.spawn()
					.await
					.unwrap();
				let mut shell = Rc::new(shell);
				let sh_mut = Rc::get_mut(&mut shell).unwrap();
				let io = IoSet {
					stdin: sh_mut.stdin.take().map(AsyncWrapW::new),
					stdout: sh_mut.stdout.take().map(AsyncWrapR::new),
					stderr: sh_mut.stderr.take().map(AsyncWrapR::new),
					wait: Box::pin(wait(shell.clone())),
				};
				user.shell = Some(shell);
				Ok(io)
			}
		}
	}
}
