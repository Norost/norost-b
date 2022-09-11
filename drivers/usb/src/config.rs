use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};

pub struct Config {
	drivers: BTreeMap<((u8, u8, u8), (u8, u8, u8)), Driver>,
}

pub struct Driver {
	pub path: Box<str>,
	/// The name the driver should get when it shares an interaction object.
	///
	/// If no name is specified, the device number is used.
	pub name: Option<Box<str>>,
}

/// Format:
/// ```
/// 	(usb-drivers
/// 		(<class> <subclass> <protocol>
/// 			(<class> <subclass> <protocol> <driver>) ..) ..)
/// 	```
pub fn parse(config: &rt::Object) -> Config {
	let size = config.seek(rt::io::SeekFrom::End(0)).unwrap();
	config.seek(rt::io::SeekFrom::Start(0)).unwrap();
	let mut buf = Vec::with_capacity(size.try_into().unwrap());
	loop {
		match config
			.read_uninit(buf.spare_capacity_mut())
			.unwrap()
			.0
			.len()
		{
			0 => break,
			n => unsafe { buf.set_len(buf.len() + n) },
		}
	}

	let mut drivers = BTreeMap::default();
	let mut cf = scf::parse2(&buf);

	let trips = |it: &mut scf::GroupsIter<'_, '_>| {
		let c = it.next_str().and_then(parse_hex_u8).unwrap();
		let sc = it.next_str().and_then(parse_hex_u8).unwrap();
		let p = it.next_str().and_then(parse_hex_u8).unwrap();
		(c, sc, p)
	};

	for mut it in cf.iter().map(|e| e.into_group().unwrap()) {
		match it.next_str().unwrap() {
			"usb-drivers" => {
				for mut it in it.map(|e| e.into_group().unwrap()) {
					let base = trips(&mut it);
					for mut it in it.map(|e| e.into_group().unwrap()) {
						let intf = trips(&mut it);
						let path = it.next_str().unwrap().into();
						let mut name = None::<Box<str>>;
						for mut it in it.map(|e| e.into_group().unwrap()) {
							match it.next_str().unwrap() {
								"name" => {
									let prev = name.replace(it.next_str().unwrap().into());
									assert!(
										prev.is_none(),
										"name already set for {:?}",
										(base, intf)
									);
									assert!(!name.as_ref().unwrap().contains('/'));
									assert!(it.next().is_none());
								}
								s => todo!("{:?}", s),
							}
						}
						let prev = drivers.insert((base, intf), Driver { path, name });
						assert!(prev.is_none(), "already specified for {:?}", (base, intf));
					}
				}
			}
			s => todo!("{:?}", s),
		}
	}

	assert!(cf.into_error().is_none());

	Config { drivers }
}

impl Config {
	pub fn get_driver(&self, base: (u8, u8, u8), interface: (u8, u8, u8)) -> Option<&Driver> {
		self.drivers.get(&(base, interface))
	}
}

fn parse_hex_u8(s: &str) -> Option<u8> {
	let f = |c| match c {
		b'0'..=b'9' => Some(c - b'0'),
		b'a'..=b'f' => Some(c - b'a' + 10),
		b'A'..=b'F' => Some(c - b'A' + 10),
		_ => None,
	};
	match s.as_bytes() {
		&[a] => f(a),
		&[a, b] => Some(f(a)? << 4 | f(b)?),
		_ => None,
	}
}
