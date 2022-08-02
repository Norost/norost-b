extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, rc::Rc};
use async_std::{
	compat::{AsyncWrapR, AsyncWrapRW, AsyncWrapW},
	net::{Ipv4Addr, TcpListener, TcpStream},
	process,
};
use clap::Parser;
use core::str;
use futures::io::{AsyncReadExt, ReadHalf, WriteHalf};
use nora_ssh::{
	auth::Auth,
	cipher,
	server::{IoSet, Server, ServerHandlers, SpawnType},
	Identifier,
};
use rand::rngs::StdRng;
use serde_derive::Deserialize;

#[derive(Debug, Deserialize)]
struct HostKeys {
	ecdsa: Option<Box<str>>,
}

#[derive(Debug, Deserialize)]
struct UserKeys {
	ed25519: Option<Box<str>>,
}

#[derive(Debug, Deserialize)]
struct UserConfig {
	keys: UserKeys,
}

#[derive(Debug, Deserialize)]
pub struct Config {
	keys: HostKeys,
	// TODO figure out a nicer mechanism for this, i.e. one that doesn't require an ever-growing
	// config file.
	users: BTreeMap<Box<str>, UserConfig>,
}

#[derive(Parser, Debug)]
struct Args {
	#[clap(value_parser)]
	config: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();

	let config = std::fs::read(&args.config).unwrap();
	let config: Config = match toml::from_slice(&config) {
		Ok(p) => p,
		Err(e) => Err(format!("ssh: failed to load config: {}", e))?,
	};

	let key = config.keys.ecdsa.expect("no keys");
	let key = std::fs::read(&*key).unwrap();
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
			Handlers {
				listener,
				users: config.users,
			},
		);

		server.start().await
	})
}

struct Handlers {
	listener: TcpListener,
	users: BTreeMap<Box<str>, UserConfig>,
}

struct User {
	shell: Option<Rc<process::Child>>,
}

enum UserKey {
	Ed25519(ed25519_dalek::PublicKey),
}

impl Handlers {
	async fn get_user_key(&self, user: &[u8], algorithm: &[u8], key: &[u8]) -> Result<UserKey, ()> {
		let user = core::str::from_utf8(user).map_err(|_| ())?;
		let user = self.users.get(user).ok_or(())?;
		let k = match algorithm {
			b"ssh-ed25519" => user.keys.ed25519.as_ref(),
			_ => None,
		}
		.ok_or(())?;
		let k = async_std::fs::read(k.clone().into_boxed_bytes())
			.await
			.0
			.map_err(|_| ())?;
		let k = str::from_utf8(&k).map_err(|_| ())?;
		let k = base85::decode(k.trim()).ok_or(())?;
		(key == k)
			.then(|| {
				let k = ed25519_dalek::PublicKey::from_bytes((&*k).try_into().unwrap());
				UserKey::Ed25519(k.unwrap())
			})
			.ok_or(())
	}
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

	async fn public_key_exists<'a>(
		&self,
		user: &'a [u8],
		_service: &'a [u8],
		algorithm: &'a [u8],
		key: &'a [u8],
	) -> Result<(), ()> {
		self.get_user_key(user, algorithm, key)
			.await
			.map(|_| ())
			.map_err(|_| ())
	}

	async fn authenticate<'a>(
		&self,
		user: &'a [u8],
		_service: &'a [u8],
		auth: Auth<'a>,
	) -> Result<Self::User, ()> {
		match auth {
			Auth::None => Err(()),
			Auth::Password(_) => Err(()),
			Auth::PublicKey {
				algorithm,
				key,
				signature,
				message,
			} => {
				use ed25519_dalek::Verifier;
				let key = self
					.get_user_key(user, algorithm, key)
					.await
					.map_err(|_| ())?;
				match key {
					UserKey::Ed25519(key) => key
						.verify(message, &signature.try_into().map_err(|_| ())?)
						.map_err(|_| ())?,
				}
				Ok(User { shell: None })
			}
		}
	}

	async fn spawn<'a>(
		&self,
		user: &'a mut Self::User,
		ty: SpawnType<'a>,
		_data: &'a [u8],
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
