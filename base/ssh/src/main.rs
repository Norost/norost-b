#![no_std]

extern crate alloc;

use alloc::boxed::Box;

use async_std::{
	env,
	net::{TcpListener, TcpStream},
	process,
};
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

fn main() -> ! {
	async_std::task::block_on(async {
		use rand::SeedableRng;
		let mut rng = rand::rngs::StdRng::seed_from_u64(0);
		let server_secret = ecdsa::SigningKey::<p256::NistP256>::random(&mut rng);

		let listener = TcpListener::bind("127.0.0.1:2222").await.unwrap();
		let server = Server::new(
			Identifier::new(b"SSH-2.0-nora_ssh example").unwrap(),
			server_secret,
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
	shell: Option<process::Child>,
}

#[async_trait::async_trait]
impl ServerHandlers for Handlers {
	type Sign = p256::NistP256;
	type Crypt = cipher::ChaCha20Poly1305;
	type Read = ReadHalf<TcpStream>;
	type Write = WriteHalf<TcpStream>;
	type User = User;
	type Stdin = process::ChildStdin;
	type Stdout = process::ChildStdout;
	type Stderr = process::ChildStderr;
	type Rng = StdRng;

	async fn accept(&self) -> (Self::Read, Self::Write) {
		self.listener.accept().await.unwrap().0.split()
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
		let wait = |child: &mut process::Child| {
			let wait = child.status();
			async move { wait.await.unwrap().code().unwrap_or(0) as u32 }
		};
		match ty {
			SpawnType::Shell => {
				let shell = "drivers/minish";
				let mut shell = process::Command::new(shell)
					.stdin(process::Stdio::piped())
					.stdout(process::Stdio::piped())
					.stderr(process::Stdio::piped())
					.spawn()
					.unwrap();
				let io = IoSet {
					stdin: shell.stdin.take(),
					stdout: shell.stdout.take(),
					stderr: shell.stderr.take(),
					wait: Box::pin(wait(&mut shell)),
				};
				user.shell = Some(shell);
				Ok(io)
			}
			SpawnType::Exec { command } => {
				let mut args = command
					.split(|c| c.is_ascii_whitespace())
					.filter(|s| !s.is_empty())
					.map(std::ffi::OsStr::from_bytes);
				let bin = args.next().unwrap();
				let mut shell = process::Command::new(bin)
					.stdin(process::Stdio::piped())
					.stdout(process::Stdio::piped())
					.stderr(process::Stdio::piped())
					.args(args)
					.spawn()
					.unwrap();
				let io = IoSet {
					stdin: shell.stdin.take(),
					stdout: shell.stdout.take(),
					stderr: shell.stderr.take(),
					wait: Box::pin(wait(&mut shell)),
				};
				user.shell = Some(shell);
				Ok(io)
			}
		}
	}
}
