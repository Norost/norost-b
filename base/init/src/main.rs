#![no_std]
#![feature(btree_drain_filter)]
#![feature(start)]

extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};
use core::time::Duration;
use norostb_rt as rt;
use rt_default as _;

#[derive(Default)]
struct Program<'a> {
	path: &'a str,
	args: Vec<&'a str>,
	env: BTreeMap<&'a str, &'a str>,
	after: Vec<&'a str>,
	file_root: Option<&'a str>,
	net_root: Option<&'a str>,
	process_root: Option<&'a str>,
	stdin: Option<&'a str>,
	stdout: Option<&'a str>,
	stderr: Option<&'a str>,
}

macro_rules! log {
	($($arg:tt)+) => {
		rt::eprintln!($($arg)+)
	};
}

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	// Open default objects
	// TODO we shouldn't hardcode the handle.
	let root = rt::Object::from_raw(0 << 24 | 0);
	let stdout @ stderr = root
		.open(b"system/log")
		.map(|o| rt::RefObject::from_raw(o.into_raw()))
		.ok();
	rt::io::set_stdout(stdout);
	rt::io::set_stderr(stderr);
	let drivers = root.open(b"drivers").unwrap();
	let process_root = root.open(b"process").unwrap();

	// Read arguments
	let cfg = drivers.open(b"init.scf").unwrap();
	let len = usize::try_from(cfg.seek(rt::io::SeekFrom::End(0)).unwrap()).unwrap();
	let (ptr, len2) = cfg.map_object(None, rt::RWX::R, 0, usize::MAX).unwrap();
	assert!(len2 >= len);
	let cfg = unsafe { core::slice::from_raw_parts(ptr.as_ptr(), len) };
	let mut it = scf::parse(cfg).map(Result::unwrap);

	use scf::Token;
	fn get_str<'a>(it: &mut dyn Iterator<Item = Token<'a>>) -> &'a str {
		match it.next() {
			Some(Token::Str(s)) => s,
			_ => panic!("expected string"),
		}
	}
	let is_begin = |it: &mut dyn Iterator<Item = Token>| match it.next() {
		Some(Token::End) => false,
		Some(Token::Begin) => true,
		_ => panic!("expected '(' or ')'"),
	};

	let mut stdin_path @ mut stdout_path @ mut stderr_path = None;
	let mut programs = Vec::new();
	'c: while let Some(tk) = it.next() {
		rt::dbg!(tk);
		assert!(tk == Token::Begin);
		match get_str(&mut it) {
			"stdin" => stdin_path = Some(get_str(&mut it)),
			"stdout" => stdout_path = Some(get_str(&mut it)),
			"stderr" => stderr_path = Some(get_str(&mut it)),
			"programs" => {
				while is_begin(&mut it) {
					let mut p = Program::default();
					p.path = get_str(&mut it);
					let mut disabled = false;
					rt::println!("program");
					'p: while is_begin(&mut it) {
						match get_str(&mut it) {
							"disabled" => disabled = true,
							"stdin" => p.stdin = Some(get_str(&mut it)),
							"stdout" => p.stdout = Some(get_str(&mut it)),
							"stderr" => p.stderr = Some(get_str(&mut it)),
							"file_root" => p.file_root = Some(get_str(&mut it)),
							"net_root" => p.net_root = Some(get_str(&mut it)),
							"process_root" => p.process_root = Some(get_str(&mut it)),
							a @ "args" | a @ "after" => loop {
								match it.next() {
									Some(Token::Str(s)) => match a {
										"args" => p.args.push(s),
										"after" => p.after.push(s),
										_ => unreachable!(),
									},
									Some(Token::End) => continue 'p,
									_ => panic!("expected ')' or string"),
								}
							},
							s => panic!("unknown property {:?}", s),
						}
						assert!(it.next() == Some(Token::End));
					}
					if !disabled {
						programs.push(p);
					}
				}
				continue 'c;
			}
			_ => panic!("unknown section"),
		}
		assert!(it.next() == Some(Token::End));
	}
	let stdin_path = stdin_path.unwrap().as_bytes();
	let stdout_path = stdout_path.unwrap().as_bytes();
	let stderr_path = stderr_path.unwrap().as_bytes();

	// Open stdin, stdout, stderr
	// Try to share stdin/out/err handles as it reduces the real amount of handles used by the
	// kernel and the servers.
	let open = |p: &[u8]| rt::RefObject::from_raw(rt::io::open(root.as_raw(), p).unwrap());
	let stdin = open(&stdin_path);
	let stdout = if stdout_path == stdin_path {
		stdin
	} else {
		open(&stdout_path)
	};
	let stderr = if stderr_path == stdin_path {
		stdin
	} else if stderr_path == stdout_path {
		stdout
	} else {
		open(&stderr_path)
	};
	rt::io::set_stdin(Some(stdout));
	rt::io::set_stdout(Some(stdout));
	rt::io::set_stderr(Some(stderr));

	// Launch programs
	log!("Launching {} programs", programs.len());
	while !programs.is_empty() {
		programs.retain(|program| {
			for f in program.after.iter() {
				// TODO open is inefficient.
				if root.open(f.as_bytes()).is_err() {
					return true;
				}
			}

			let open = |base: &Option<&str>| match base {
				None => None,
				Some("") => Some(None),
				Some(path) => Some(Some(root.open(path.as_bytes()).unwrap())),
			};
			fn select<'a>(
				base: &'a Option<Option<rt::Object>>,
				default: &'a rt::Object,
			) -> Option<rt::RefObject<'a>> {
				match base {
					None => None,
					Some(None) => Some(default.into()),
					Some(Some(base)) => Some(base.into()),
				}
			}

			let t = open(&program.stdin);
			let stdin = select(&t, &stdin).unwrap_or(stdin);
			let t = open(&program.stdout);
			let stdout = select(&t, &stdout).unwrap_or(stdout);
			let t = open(&program.stderr);
			let stderr = select(&t, &stderr).unwrap_or(stderr);
			let t = open(&program.file_root);
			let file_root = select(&t, &root);
			let t = open(&program.net_root);
			let net_root = select(&t, &root);
			let t = open(&program.process_root);
			let proc_root = select(&t, &process_root);

			let binary = drivers
				.open(program.path.as_bytes())
				.unwrap_or_else(|e| panic!("failed to open {:?}: {:?}", &program.path, e));
			let r = rt::Process::new(
				&process_root,
				&binary,
				[
					(rt::args::ID_STDIN, stdin),
					(rt::args::ID_STDOUT, stdout),
					(rt::args::ID_STDERR, stderr),
				]
				.into_iter()
				.chain(file_root.map(|r| (rt::args::ID_FILE_ROOT, r)))
				.chain(net_root.map(|r| (rt::args::ID_NET_ROOT, r)))
				.chain(proc_root.map(|r| (rt::args::ID_PROCESS_ROOT, r))),
				[program.path]
					.into_iter()
					.chain(program.args.iter().copied()),
				program.env.iter(),
			);
			match r {
				Ok(_) => log!("Launched {:?}", program.path),
				Err(e) => log!("Failed to launch {:?}: {:?}", program.path, e),
			}

			false
		});
		// TODO poll for changes instead of busy waiting.
		rt::thread::sleep(Duration::from_millis(1));
	}

	log!("Finished init");

	rt::exit(0);
}
