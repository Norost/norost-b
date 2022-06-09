#![feature(btree_drain_filter)]

use norostb_rt as rt;
use serde_derive::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct Programs {
	program: BTreeMap<String, Program>,
	stdin: String,
	stdout: String,
	stderr: String,
}

#[derive(Debug, Deserialize)]
struct Program {
	disabled: Option<bool>,
	path: String,
	args: Option<Vec<String>>,
	env: Option<BTreeMap<String, String>>,
	after: Option<Vec<String>>,
	file_root: Option<String>,
	net_root: Option<String>,
	process_root: Option<String>,
	stdin: Option<String>,
	stdout: Option<String>,
	stderr: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Open default objects
	// TODO we shouldn't hardcode the handle.
	let root = rt::Object::from_raw(0);
	let drivers = root.open(b"drivers").unwrap();
	let process_root = root.open(b"process").unwrap();

	// Read arguments
	println!("Parsing drivers/init.toml");
	let args = drivers.open(b"init.toml").unwrap();
	let args_len = args.seek(rt::io::SeekFrom::End(0)).unwrap();
	args.seek(rt::io::SeekFrom::Start(0)).unwrap();
	let mut args_buf = Vec::new();
	args_buf.resize(args_len.try_into().unwrap(), 0);
	args.read(&mut args_buf).unwrap();
	let Programs {
		program: mut programs,
		stdin: stdin_path,
		stdout: stdout_path,
		stderr: stderr_path,
	} = match toml::from_slice(&args_buf) {
		Ok(p) => p,
		Err(e) => {
			eprintln!("{}", e);
			std::process::exit(1);
		}
	};

	// Open stdin, stdout, stderr
	// Try to share stdin/out/err handles as it reduces the real amount of handles used by the
	// kernel and the servers.
	let open = |p: &str| {
		let t = rt::io::open(root.as_raw(), p.as_bytes().into(), 0);
		rt::RefObject::from_raw(rt::io::block_on(t).unwrap().1)
	};
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

	programs.retain(|_, p| !p.disabled.unwrap_or(false));

	// Launch programs
	println!("Launching {} programs", programs.len());
	while !programs.is_empty() {
		programs.retain(|name, program| {
			for f in program.after.as_ref().iter().flat_map(|i| i.iter()) {
				// TODO open is inefficient.
				if root.open(f.as_bytes()).is_err() {
					return true;
				}
			}

			let open = |base: &Option<String>| match base.as_deref() {
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
					Some(None) => Some(default.as_ref_object()),
					Some(Some(base)) => Some(base.as_ref_object()),
				}
			}

			let t = open(&program.stdin);
			let stdin = select(&t, &stdin).unwrap_or(stdin.as_ref_object());
			let t = open(&program.stdout);
			let stdout = select(&t, &stdout).unwrap_or(stdout.as_ref_object());
			let t = open(&program.stderr);
			let stderr = select(&t, &stderr).unwrap_or(stderr.as_ref_object());
			let t = open(&program.file_root);
			let file_root = select(&t, &root);
			let t = open(&program.net_root);
			let net_root = select(&t, &root);
			let t = open(&program.process_root);
			let proc_root = select(&t, &process_root);

			let binary = drivers.open(program.path.as_bytes()).unwrap();
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
				[name]
					.into_iter()
					.chain(program.args.iter().flat_map(|i| i.iter()))
					.map(|s| s.as_bytes()),
				program
					.env
					.iter()
					.flat_map(|i| i.iter())
					.map(|(k, v)| (k.as_bytes(), v.as_bytes())),
			);
			match r {
				Ok(_) => println!("Launched {:?}", name),
				Err(e) => println!("Failed to launch {:?}: {:?}", name, e),
			}

			false
		});
		// TODO poll for changes instead of busy waiting.
		rt::thread::sleep(std::time::Duration::from_millis(1));
	}

	println!("Finished init");

	Ok(())
}
