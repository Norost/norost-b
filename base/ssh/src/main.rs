extern crate alloc;

use {
	alloc::{boxed::Box, rc::Rc},
	async_std::{
		compat::{AsyncWrapR, AsyncWrapRW, AsyncWrapW},
		net::{Ipv4Addr, TcpListener, TcpStream},
		process, AsyncObject,
	},
	core::str,
	futures::io::{AsyncReadExt, ReadHalf, WriteHalf},
	nora_ssh::{
		auth::Auth,
		cipher,
		server::{IoSet, Server, ServerHandlers, SpawnType},
		Identifier,
	},
	rand::rngs::StdRng,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let config = parse_config();

	async_std::task::block_on(async {
		let addr = (Ipv4Addr::UNSPECIFIED, 22);
		let listener = TcpListener::bind(addr).await.unwrap();
		let server = Server::new(
			Identifier::new(b"SSH-2.0-nora_ssh example").unwrap(),
			config.host_keys.ecdsa.expect("no host key"),
			Handlers { listener, userdb: config.userdb },
		);

		server.start().await
	})
}

struct Handlers {
	listener: TcpListener,
	userdb: rt::Object,
}

struct User {
	shell: Option<Rc<process::Process>>,
	context: rt::Object,
}

impl User {
	async fn spawn_shell(
		&mut self,
		bin: &[u8],
	) -> rt::io::Result<
		IoSet<AsyncWrapW<AsyncObject>, AsyncWrapR<AsyncObject>, AsyncWrapR<AsyncObject>>,
	> {
		let wait =
			|child: Rc<process::Process>| async move { child.wait().await.unwrap().code as u32 };
		let (stdin, stdin_shr) = rt::Object::new(rt::NewObject::Pipe)?;
		let (stdout_shr, stdout) = rt::Object::new(rt::NewObject::Pipe)?;
		let (stderr_shr, stderr) = rt::Object::new(rt::NewObject::Pipe)?;
		let mut b = process::Builder::new().await?;
		b.set_binary_by_name(Vec::from(bin)).await.0?;
		b.add_object(b"in", stdin_shr.into()).await.0?;
		b.add_object(b"out", stdout_shr.into()).await.0?;
		b.add_object(b"err", stderr_shr.into()).await.0?;
		b.add_object_raw(b"file", self.context.as_raw()).await?;
		let shell = b.spawn().await?;
		let shell = Rc::new(shell);
		let io = IoSet {
			stdin: Some(AsyncWrapW::new(AsyncObject::from(stdin))),
			stdout: Some(AsyncWrapR::new(AsyncObject::from(stdout))),
			stderr: Some(AsyncWrapR::new(AsyncObject::from(stderr))),
			wait: Box::pin(wait(shell.clone())),
		};
		self.shell = Some(shell);
		Ok(io)
	}
}

enum UserKey {
	Ed25519(ed25519_dalek::PublicKey),
}

impl Handlers {
	async fn get_user_key(&self, user: &[u8], algorithm: &[u8], key: &[u8]) -> Result<UserKey, ()> {
		if algorithm != b"ssh-ed25519" {
			return Err(());
		}

		let mut path = Vec::from(&b"open/"[..]);
		path.extend_from_slice(user);
		path.extend_from_slice(b"/cfg/ssh.scf");
		let cfg = self
			.userdb
			.open(&path)
			.map_err(|_| ())?
			.read_file_all()
			.map_err(|_| ())?;
		let mut cf = scf::parse2(&cfg);
		let mut kk = None;
		for item in cf.iter() {
			let mut it = item.into_group().unwrap();
			assert!(it.next_str() == Some("authorized"));
			for item in it {
				let mut it = item.into_group().unwrap();
				assert!(it.next_str() == Some("ssh-ed25519"));
				let s = it.next_str().unwrap();
				let k = n85::decode_vec(s.as_ref()).unwrap();
				if &k == key {
					kk = Some(k)
				}
				assert!(it.next().is_none());
			}
		}
		kk.and_then(|k| (key == &k).then(|| k))
			.map(|k| {
				let k = ed25519_dalek::PublicKey::from_bytes((&*k).try_into().unwrap());
				UserKey::Ed25519(k.unwrap())
			})
			.ok_or(())
	}

	async fn get_user_context(&self, user: &[u8]) -> Result<rt::Object, ()> {
		let mut path = Vec::from(&b"auth/"[..]);
		path.extend_from_slice(user);
		self.userdb.open(&path).map_err(|e| todo!("{:?}", e))
	}
}

#[async_trait::async_trait(?Send)]
impl ServerHandlers for Handlers {
	type Sign = p256::NistP256;
	type Crypt = cipher::ChaCha20Poly1305;
	type Read = ReadHalf<AsyncWrapRW<TcpStream>>;
	type Write = WriteHalf<AsyncWrapRW<TcpStream>>;
	type User = User;
	type Stdin = AsyncWrapW<AsyncObject>;
	type Stdout = AsyncWrapR<AsyncObject>;
	type Stderr = AsyncWrapR<AsyncObject>;
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
			Auth::PublicKey { algorithm, key, signature, message } => {
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
				Ok(User { shell: None, context: self.get_user_context(user).await? })
			}
		}
	}

	async fn spawn<'a>(
		&self,
		user: &'a mut Self::User,
		ty: SpawnType<'a>,
		_data: &'a [u8],
	) -> Result<IoSet<Self::Stdin, Self::Stdout, Self::Stderr>, ()> {
		match ty {
			SpawnType::Shell => user.spawn_shell(b"drivers/minish").await.map_err(|_| ()),
			SpawnType::Exec { command } => {
				let mut args = command
					.split(|c| c.is_ascii_whitespace())
					.filter(|s| !s.is_empty());
				let bin = args.next().unwrap();
				let bin = match bin {
					b"scp" => b"drivers/scp", // TODO do this properly with PATH env or whatever
					b => b,
				};
				user.spawn_shell(bin).await.map_err(|_| ())
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
struct Config {
	host_keys: HostKeys,
	userdb: rt::Object,
}

#[derive(Default)]
struct UserConfig {}

fn parse_config() -> Config {
	let mut cfg @ mut cfg_secret @ mut userdb = None;
	for (n, o) in rt::args::handles() {
		match n {
			b"cfg" => cfg = Some(o),
			b"cfg_secret" => cfg_secret = Some(o),
			b"userdb" => userdb = Some(o),
			_ => {}
		}
	}
	// cfg is currently unused, but do ensure it is defined.
	cfg.expect("cfg not defined");
	let cfg_secret = cfg_secret
		.expect("cfg_secret not defined")
		.read_file_all()
		.unwrap();
	let userdb = userdb.expect("userdb is not defined");

	let mut token = None;
	let mut host_keys = HostKeys::default();

	let mut cf = scf::parse2(&cfg_secret);
	for item in cf.iter() {
		let mut it = item.into_group().unwrap();
		match it.next_str().expect("expected section name") {
			"keys" => {
				for item in it {
					let mut it = item.into_group().unwrap();
					let algo = it.next_str().expect("expected key algorithm");
					let path = it.next_str().expect("expected key path");
					let prev = match algo {
						"ecdsa" => host_keys.ecdsa.replace(read_key_ecdsa(path)),
						s => panic!("unknown key algorithm {:?}", s),
					};
					assert!(prev.is_none(), "key defined twice");
					assert!(it.next().is_none());
				}
			}
			"userdb" => {
				let prev = token.replace(it.next_str().expect("expected token"));
				assert!(prev.is_none(), "token defined twice");
				assert!(it.next().is_none());
			}
			s => panic!("unknown section {:?}", s),
		}
	}

	// Authenticate to userdb
	let token = token.expect("token for userdb");
	let path = format!("service/ssh/{}", token);
	let userdb = userdb
		.open(path.as_bytes())
		.expect("failed to authenticate to userdb");

	Config { host_keys, userdb }
}

fn read_key_ecdsa(key: &str) -> ecdsa::SigningKey<p256::NistP256> {
	let mut buf = [0; 32];
	let l = n85::decode(key.as_ref(), &mut buf).expect("invalid key");
	assert!(l == buf.len(), "invalid key");
	ecdsa::SigningKey::from_bytes(&mut buf).expect("invalid key")
}
