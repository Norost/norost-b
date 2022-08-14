use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use nora_scf::Token;

pub struct Config {
	drivers: BTreeMap<((u8, u8, u8), (u8, u8, u8)), Box<[u8]>>,
}

/// Format:
/// ```
///	(usb-drivers
///		(<class> <subclass> <protocol>
///			(<class> <subclass> <protocol> <driver>) ..) ..)
///	```
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
	let mut it = nora_scf::parse(&buf);

	let s = for<'a, 'b> |it: &'b mut nora_scf::Iter<'a>| -> &'a [u8] {
		it.next().unwrap().unwrap().into_str().unwrap()
	};

	let trips = |it: &mut _| {
		let c = parse_hex_u8(s(it)).unwrap();
		let sc = parse_hex_u8(s(it)).unwrap();
		let p = parse_hex_u8(s(it)).unwrap();
		(c, sc, p)
	};

	while let Some(tk) = it.next().map(Result::unwrap) {
		match tk {
			Token::Begin => {
				assert_eq!(it.next(), Some(Ok(Token::Str(b"usb-drivers"))));
				match it.next().unwrap().unwrap() {
					Token::Begin => {
						let base = trips(&mut it);
						loop {
							match it.next().unwrap().unwrap() {
								Token::Begin => {
									let intf = trips(&mut it);
									let driver = s(&mut it);
									let prev = drivers.insert((base, intf), Box::from(driver));
									assert!(
										prev.is_none(),
										"already specified for {:?}",
										(base, intf)
									);
									assert_eq!(it.next(), Some(Ok(Token::End)));
								}
								Token::End => break,
								Token::Str(_) => panic!("unexpected string"),
							}
						}
						assert_eq!(it.next(), Some(Ok(Token::End)));
					}
					Token::End => {}
					Token::Str(_) => panic!("unexpected string"),
				}
			}
			Token::End => panic!("unexpected ')'"),
			Token::Str(_) => panic!("unexpected string"),
		}
	}

	Config { drivers }
}

impl Config {
	pub fn get_driver(&self, base: (u8, u8, u8), interface: (u8, u8, u8)) -> Option<&[u8]> {
		self.drivers.get(&(base, interface)).map(|b| &**b)
	}
}

fn parse_hex_u8(s: &[u8]) -> Option<u8> {
	let f = |c| match c {
		b'0'..=b'9' => Some(c - b'0'),
		b'a'..=b'f' => Some(c - b'a' + 10),
		b'A'..=b'F' => Some(c - b'A' + 10),
		_ => None,
	};
	match s {
		&[a] => f(a),
		&[a, b] => Some(f(a)? << 4 | f(b)?),
		_ => None,
	}
}
