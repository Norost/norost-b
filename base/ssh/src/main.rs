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

#[derive(Parser, Debug)]
struct Args {
	#[clap(value_parser)]
	config: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();

	let config = parse_config(&args.config);

	async_std::task::block_on(async {
		let addr = (Ipv4Addr::UNSPECIFIED, 22);
		let listener = TcpListener::bind(addr).await.unwrap();
		let server = Server::new(
			Identifier::new(b"SSH-2.0-nora_ssh example").unwrap(),
			config.host_keys.ecdsa.expect("no host key"),
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
	users: BTreeMap<Box<[u8]>, UserConfig>,
}

struct User {
	shell: Option<Rc<process::Child>>,
}

enum UserKey {
	Ed25519(ed25519_dalek::PublicKey),
}

impl Handlers {
	async fn get_user_key(&self, user: &[u8], algorithm: &[u8], key: &[u8]) -> Result<UserKey, ()> {
		let user = self.users.get(user).ok_or(())?;
		let k = match algorithm {
			b"ssh-ed25519" => user.ed25519.as_ref(),
			_ => None,
		}
		.ok_or(())?;
		let k = async_std::fs::read(k.clone()).await.0.map_err(|_| ())?;
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

#[derive(Default)]
struct HostKeys {
	ecdsa: Option<ecdsa::SigningKey<p256::NistP256>>,
}

// Just as an example
//
// Realistically, the server should load the key only when appropriate
#[derive(Default)]
struct Config {
	host_keys: HostKeys,
	users: BTreeMap<Box<[u8]>, UserConfig>,
}

#[derive(Default)]
struct UserConfig {
	ed25519: Option<Box<[u8]>>,
}

fn parse_config(path: &str) -> Config {
	let cfg = std::fs::read(path).unwrap();
	let mut it = scf::parse(&cfg).map(Result::unwrap);

	let mut cfg = Config::default();

	use scf::Token;
	let is_begin = |it: &mut dyn Iterator<Item = Token<'_>>| match it.next() {
		Some(Token::Begin) => true,
		Some(Token::End) => false,
		_ => panic!("expected '(' or ')'"),
	};
	fn get_str<'a>(it: &mut dyn Iterator<Item = Token<'a>>) -> Option<&'a str> {
		it.next().and_then(|o| o.into_str())
	};

	while let Some(tk) = it.next() {
		assert!(tk == Token::Begin);
		match get_str(&mut it).expect("expected section name") {
			"keys" => {
				while is_begin(&mut it) {
					let algo = get_str(&mut it).expect("expected key algorithm");
					let path = get_str(&mut it).expect("expected key path");
					let prev = match algo {
						"ecdsa" => cfg.host_keys.ecdsa.replace(read_key_ecdsa(path)),
						s => panic!("unknown key algorithm {:?}", s),
					};
					assert!(prev.is_none(), "key defined twice");
					assert!(it.next() == Some(Token::End));
				}
			}
			"users" => {
				while is_begin(&mut it) {
					let user = get_str(&mut it).expect("expected user name");
					let mut c = UserConfig::default();
					while is_begin(&mut it) {
						let algo = get_str(&mut it).expect("expected key algorithm");
						let path = get_str(&mut it).expect("expected key path");
						let prev = match algo {
							"ed25519" => c.ed25519.replace(path.as_bytes().into()),
							s => panic!("unknown key algorithm {:?}", s),
						};
						assert!(prev.is_none(), "key defined twice");
						assert!(it.next() == Some(Token::End));
					}
					let prev = cfg.users.insert(user.as_bytes().into(), c);
					assert!(prev.is_none(), "user defined twice");
				}
			}
			s => panic!("unknown section {:?}", s),
		}
	}

	cfg
}

fn read_key(path: &str) -> Vec<u8> {
	let key = std::fs::read(&*path).unwrap();
	let key = std::str::from_utf8(&key).expect("invalid key");
	let key = key.trim();
	base85::decode(key).expect("invalid key")
}

fn read_key_ecdsa(path: &str) -> ecdsa::SigningKey<p256::NistP256> {
	let key = read_key(path);
	ecdsa::SigningKey::from_bytes(&key).expect("invalid key")
}
