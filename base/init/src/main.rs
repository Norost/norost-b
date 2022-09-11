#![no_std]
#![feature(btree_drain_filter)]
#![feature(start)]
#![feature(result_option_inspect)]

extern crate alloc;

use {alloc::vec::Vec, core::time::Duration, norostb_rt as rt, rt_default as _};

const SYSLOG: &str = "syslog/write";

#[derive(Default)]
struct Program<'a> {
	path: &'a str,
	args: Vec<&'a str>,
	env: Vec<(&'a str, &'a str)>,
	after: Vec<&'a str>,
	open: Vec<(&'a str, Vec<&'a str>)>,
	create: Vec<(&'a str, Vec<&'a str>)>,
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
	let stderr = root
		.open(SYSLOG.as_ref())
		.map(|o| rt::RefObject::from_raw(o.into_raw()))
		.ok();
	rt::io::set_stderr(stderr);
	let drivers = root.open(b"drivers").unwrap();
	let process_root = root.open(b"process").unwrap();

	// Read arguments
	let cfg = drivers.open(b"init.scf").unwrap();
	let len = usize::try_from(cfg.seek(rt::io::SeekFrom::End(0)).unwrap()).unwrap();
	let (ptr, len2) = cfg.map_object(None, rt::RWX::R, 0, usize::MAX).unwrap();
	assert!(len2 >= len);
	let cfg = unsafe { core::slice::from_raw_parts(ptr.as_ptr(), len) };
	let mut cf = scf::parse2(cfg);

	let mut programs = Vec::new();
	for item in cf.iter() {
		let mut it = item.into_group().unwrap();
		match it.next_str().unwrap() {
			"programs" => {
				for item in it {
					let mut it = item.into_group().unwrap();
					let mut p = Program::default();
					p.path = it.next_str().unwrap();
					let mut disabled = false;
					for item in it {
						let mut it = item.into_group().unwrap();
						match it.next_str().unwrap() {
							"disabled" => {
								disabled = true;
								assert!(it.next().is_none());
							}
							"env" => {
								for item in it {
									let mut it = item.into_group().unwrap();
									let key = it.next_str().unwrap();
									let val = it.next_str().unwrap();
									assert!(it.next().is_none());
									p.env.push((key, val));
								}
							}
							s @ "open" | s @ "create" => {
								for item in it {
									let mut it = item.into_group().unwrap();
									let name = it.next_str().unwrap();
									let path = it.map(|e| e.into_str().unwrap()).collect();
									match s {
										"open" => &mut p.open,
										"create" => &mut p.create,
										_ => unreachable!(),
									}
									.push((name, path));
								}
							}
							a @ "args" | a @ "after" => {
								*match a {
									"args" => &mut p.args,
									"after" => &mut p.after,
									_ => unreachable!(),
								} = it.map(|e| e.into_str().unwrap()).collect();
							}
							s => panic!("unknown property {:?}", s),
						}
					}
					if !disabled {
						programs.push(p);
					}
				}
			}
			_ => panic!("unknown section"),
		}
	}

	// Add stderr by default, as it is used for panic & other output
	for p in programs.iter_mut() {
		if !p
			.open
			.iter()
			.chain(&*p.create)
			.find(|(n, _)| *n == "err")
			.is_some()
		{
			p.open.push(("err", Vec::from([SYSLOG])));
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
				let mut open_create = |name: &str, path: &[&str], create| {
					// FIXME bug in Root, probably
					if path == &[""] {
						b.add_object(name.as_ref(), &root)?;
					} else {
						let (last, path) = path.split_last().unwrap();
						let mut sto = None;
						let mut obj = &root;
						for p in path.iter().map(|p| p.as_bytes()) {
							let o = obj.open(p)?;
							obj = &*sto.insert(o);
						}
						let obj = if create {
							obj.create(last.as_bytes())
						} else {
							obj.open(last.as_bytes())
						}?;
						b.add_object(name.as_ref(), &obj)?;
					}
					Ok(())
				};
				for (name, path) in &program.open {
					open_create(name, &path, false)
						.inspect_err(|e: &rt::Error| log!("Failed to open {:?}: {:?}", path, e))?;
				}
				for (name, path) in &program.create {
					open_create(name, &path, true).inspect_err(|e: &rt::Error| {
						log!("Failed to create {:?}: {:?}", path, e)
					})?;
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
