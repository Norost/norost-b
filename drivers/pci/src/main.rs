#![no_std]
#![feature(closure_lifetime_binder)]
#![feature(start)]

extern crate alloc;

use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let file_root = rt::io::file_root().unwrap();
	let cfg = load_config();
	let pci = file_root.open(b"pci").unwrap();
	let list = pci.open(b"xinfo").unwrap();
	loop {
		let mut b = [0; 32];
		let n = list.read(&mut b).unwrap();
		if n == 0 {
			break;
		}
		let b = core::str::from_utf8(&b[..n]).unwrap();
		let (loc, id_class) = b.split_once(' ').unwrap();
		let (id, class) = id_class.split_once(' ').unwrap();
		let (v, d) = id.split_once(':').unwrap();
		let (v, d) = (parse_hex_u16(v).unwrap(), parse_hex_u16(d).unwrap());
		let mut it = class.split('/');
		let mut f = || parse_hex_u8(it.next().unwrap()).unwrap();
		let class = (f(), f(), f());
		assert!(it.next().is_none());

		let process_root = rt::io::process_root().unwrap();
		if let Some(d) = cfg
			.drivers_by_id
			.get(&(v, d))
			.or_else(|| cfg.drivers_by_class.get(&class))
		{
			if let Err(e) = (|| {
				let mut b = rt::process::Builder::new()?;
				b.set_binary_by_name(d.path.as_bytes())?;
				b.add_args([loc, d.name.as_deref().unwrap_or(loc)])?;
				if let Some(o) = rt::io::stderr() {
					b.add_object(b"err", &o)?;
				}
				b.add_object(b"file", &file_root)?;
				b.add_object(b"process", &process_root)?;
				b.add_object(b"pci", &pci.open(loc.as_ref())?)?;
				b.spawn()
			})() {
				rt::eprintln!("failed to launch driver {:?}: {:?}", d.path, e);
			} else {
				rt::eprintln!("launched driver {:?} for {}", d.path, loc);
			}
		} else {
			rt::eprintln!("no driver for {:04x}:{:04x}", v, d);
		}
	}
	todo!();
}

#[derive(Default)]
struct Config {
	drivers_by_id: BTreeMap<(u16, u16), Driver>,
	drivers_by_class: BTreeMap<(u8, u8, u8), Driver>,
}

struct Driver {
	path: Box<str>,
	name: Option<Box<str>>,
}

fn load_config() -> Config {
	let file_root = rt::io::file_root().unwrap();
	let cfg = rt::args::args().skip(1).next().expect("pci object");
	let cfg = file_root.open(cfg).unwrap();
	let len = cfg
		.seek(rt::io::SeekFrom::End(0))
		.unwrap()
		.try_into()
		.unwrap();
	cfg.seek(rt::io::SeekFrom::Start(0)).unwrap();
	let mut buf = Vec::with_capacity(len);
	while buf.len() < len {
		let l = cfg.read_uninit(buf.spare_capacity_mut()).unwrap().0.len();
		unsafe { buf.set_len(buf.len() + l) }
	}
	let mut cfg = Config::default();
	let mut cf = scf::parse2(&buf);

	let parse_driver = |mut it: scf::GroupsIter<'_, '_>| {
		let path = it.next_str().expect("expected driver path");
		let mut name = None;
		for item in it {
			let mut it = item.into_group().unwrap();
			match it.next_str().expect("expected property name") {
				"name" => {
					let prev = name.replace(it.next_str().expect("expected property name"));
					assert!(prev.is_none(), "multiple names for driver");
				}
				s => panic!("unknown property {:?}", s),
			}
		}
		Driver {
			path: path.into(),
			name: name.map(|n| n.into()),
		}
	};

	for item in cf.iter() {
		let mut it = item.into_group().unwrap();
		match it.next_str().expect("section name") {
			"id" => {
				for item in it {
					let mut it = item.into_group().unwrap();
					let vendor = parse_hex_u16(it.next_str().expect("expected vendor ID"))
						.expect("invalid vendor ID");
					for item in it {
						let mut it = item.into_group().unwrap();
						let device = parse_hex_u16(it.next_str().expect("expected device ID"))
							.expect("invalid device ID");
						let prev = cfg.drivers_by_id.insert((vendor, device), parse_driver(it));
						assert!(
							prev.is_none(),
							"multiple drivers for {:04x}:{:04x}",
							vendor,
							device
						);
					}
				}
			}
			"class" => {
				for item in it {
					let mut it = item.into_group().unwrap();
					let class = parse_hex_u8(it.next_str().expect("expected class"))
						.expect("invalid class");
					let subclass = parse_hex_u8(it.next_str().expect("expected subclass"))
						.expect("invalid subclass");
					let interface = parse_hex_u8(it.next_str().expect("expected interface"))
						.expect("invalid interface");
					let prev = cfg
						.drivers_by_class
						.insert((class, subclass, interface), parse_driver(it));
					assert!(
						prev.is_none(),
						"multiple drivers for {:02x} {:02x} {:02x}",
						class,
						subclass,
						interface
					);
				}
			}
			s => panic!("unknown section {:?}", s),
		}
	}

	cfg
}

fn parse_hex_u8(n: &str) -> Option<u8> {
	let f = |n| {
		Some(match n {
			b'0'..=b'9' => n - b'0',
			b'a'..=b'f' => n - b'a' + 10,
			b'A'..=b'F' => n - b'A' + 10,
			_ => return None,
		})
	};
	match n.as_bytes() {
		&[a] => f(a),
		&[a, b] => Some(f(a)? << 4 | f(b)?),
		_ => None,
	}
}

fn parse_hex_u16(n: &str) -> Option<u16> {
	let f = |n| parse_hex_u8(n).map(u16::from);
	if n.len() <= 2 {
		f(n)
	} else if n.len() <= 4 {
		Some(f(n.get(..2)?)? << 8 | f(n.get(2..)?)?)
	} else {
		None
	}
}
