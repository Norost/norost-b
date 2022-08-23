#![no_std]
#![feature(btree_drain_filter)]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::time::Duration;
use norostb_rt as rt;
use rt_default as _;

#[derive(Default)]
struct Program<'a> {
	path: &'a str,
	args: Vec<&'a str>,
	env: Vec<(&'a str, &'a str)>,
	after: Vec<&'a str>,
	objects: Vec<(&'a str, &'a str)>,
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
	let start_time = rt::time::Monotonic::now();

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

	let mut stdout_path @ mut stderr_path = None;
	let mut programs = Vec::new();
	'c: while let Some(tk) = it.next() {
		assert!(tk == Token::Begin);
		match get_str(&mut it) {
			"stdout" => stdout_path = Some(get_str(&mut it)),
			"stderr" => stderr_path = Some(get_str(&mut it)),
			"programs" => {
				while is_begin(&mut it) {
					let mut p = Program::default();
					p.path = get_str(&mut it);
					let mut disabled = false;
					while is_begin(&mut it) {
						match get_str(&mut it) {
							"disabled" => {
								disabled = true;
								assert!(it.next() == Some(Token::End));
							}
							"env" => {
								while is_begin(&mut it) {
									let key = get_str(&mut it);
									let val = get_str(&mut it);
									assert!(it.next() == Some(Token::End));
									p.env.push((key, val));
								}
							}
							"objects" => {
								while is_begin(&mut it) {
									let name = get_str(&mut it);
									let path = get_str(&mut it);
									assert!(it.next() == Some(Token::End));
									p.objects.push((name, path));
								}
							}
							a @ "args" | a @ "after" => loop {
								match it.next() {
									Some(Token::Str(s)) => match a {
										"args" => p.args.push(s),
										"after" => p.after.push(s),
										_ => unreachable!(),
									},
									Some(Token::End) => break,
									_ => panic!("expected ')' or string"),
								}
							},
							s => panic!("unknown property {:?}", s),
						}
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
	let stdout_path = stdout_path.unwrap();
	let stderr_path = stderr_path.unwrap();

	let open = |p: &[u8]| rt::RefObject::from_raw(rt::io::open(root.as_raw(), p).unwrap());
	let stdout = open(stdout_path.as_bytes());
	let stderr = open(stderr_path.as_bytes());
	rt::io::set_stdin(Some(stdout));
	rt::io::set_stdout(Some(stdout));
	rt::io::set_stderr(Some(stderr));

	// Add stderr by default, as it is used for panic & other output
	for p in programs.iter_mut() {
		if !p.objects.iter().find(|(n, _)| *n == "err").is_some() {
			p.objects.push(("err", stderr_path));
		}
	}

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

			let r = (|| {
				let bin = drivers.open(program.path.as_bytes())?;
				let mut b = rt::process::Builder::new_with(&process_root)?;
				b.set_binary(&bin)?;
				for (name, path) in &program.objects {
					// FIXME bug in Root, probably
					if *path == "" {
						b.add_object(name.as_ref(), &root)?;
					} else {
						let obj = root.open(path.as_ref()).unwrap();
						b.add_object(name.as_ref(), &obj)?;
					}
				}
				b.add_args(&[program.path])?;
				b.add_args(&program.args)?;
				// TODO env
				b.spawn()
			})();
			match r {
				Ok(_) => log!("Launched {:?}", program.path),
				Err(e) => log!("Failed to launch {:?}: {:?}", program.path, e),
			}

			false
		});
		// TODO poll for changes instead of busy waiting.
		rt::thread::sleep(Duration::from_millis(1));
	}

	let t = rt::time::Monotonic::now().saturating_duration_since(start_time);
	log!("Finished init in {:?}", t);

	rt::exit(0);
}
