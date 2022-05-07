#![feature(btree_drain_filter)]

use norostb_rt as rt;
use serde_derive::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct Programs {
	program: BTreeMap<String, Program>,
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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Open default objects
	// TODO we shouldn't hardcode the handle.
	let root = rt::Object::from_raw(0);
	let stdin = rt::io::open(root.as_raw(), b"uart/0").unwrap();
	let stdout @ stderr = rt::io::open(root.as_raw(), b"system/log").unwrap();
	let stdin = rt::RefObject::from_raw(stdin);
	let stdout = rt::RefObject::from_raw(stdout);
	let stderr = rt::RefObject::from_raw(stderr);
	rt::io::set_stdout(Some(stdout));
	rt::io::set_stderr(Some(stderr));
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
	} = toml::from_slice(&args_buf)?;

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

			let file_root = match program.file_root.as_deref() {
				None => None,
				Some("") => Some(None),
				Some(path) => Some(Some(root.open(path.as_bytes()).unwrap())),
			};
			let file_root = match &file_root {
				None => None,
				Some(None) => Some(root.as_ref_object()),
				Some(Some(r)) => Some(r.as_ref_object()),
			};

			let net_root = match program.net_root.as_deref() {
				None => None,
				Some("") => Some(None),
				Some(path) => Some(Some(root.open(path.as_bytes()).unwrap())),
			};
			let net_root = match &net_root {
				None => None,
				Some(None) => Some(root.as_ref_object()),
				Some(Some(r)) => Some(r.as_ref_object()),
			};

			let proc_root = match program.process_root.as_deref() {
				None => None,
				Some("") => Some(None),
				Some(path) => Some(Some(root.open(path.as_bytes()).unwrap())),
			};
			let proc_root = match &proc_root {
				None => None,
				Some(None) => Some(root.as_ref_object()),
				Some(Some(r)) => Some(r.as_ref_object()),
			};

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
