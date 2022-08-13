//! # Configuration parser

extern crate alloc;

use crate::{KeyCode, SpecialKeyCode};
use alloc::{boxed::Box, collections::BTreeMap};
use core::{fmt, str};

pub struct RawMap {
	/// Map for single-byte scancodes.
	map_single: [Option<KeyCode>; 256],
	/// Map for double-byte scancodes.
	map_double: BTreeMap<[u8; 2], KeyCode>,
	/// Map for arbitrary-length scancodes over 2 bytes.
	map_long: BTreeMap<Box<[u8]>, KeyCode>,
}

impl RawMap {
	/// Get the corresponding keycode for a scancode.
	pub fn get(&self, scancode: &[u8]) -> Option<KeyCode> {
		match scancode {
			&[a] => self.map_single[usize::from(a)],
			&[a, b] => self.map_double.get(&[a, b]).copied(),
			s => self.map_long.get(s).copied(),
		}
	}
}

impl Default for RawMap {
	fn default() -> Self {
		Self {
			map_single: [None; 256],
			map_double: Default::default(),
			map_long: Default::default(),
		}
	}
}

impl fmt::Debug for RawMap {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_map();
		f.entries(
			self.map_single
				.iter()
				.enumerate()
				.filter_map(|(k, v)| v.map(|v| (k, v))),
		);
		f.entries(&self.map_double);
		f.entries(&self.map_long);
		f.finish()
	}
}

#[derive(Default, Debug)]
pub struct Config {
	raw: RawMap,
	translate_caps: BTreeMap<KeyCode, KeyCode>,
	translate_altgr: BTreeMap<KeyCode, KeyCode>,
	translate_altgr_caps: BTreeMap<KeyCode, KeyCode>,
}

#[derive(Clone, Copy, Debug)]
pub struct Modifiers {
	pub altgr: bool,
	pub caps: bool,
	pub num: bool,
}

impl Config {
	pub fn raw(&self, scancode: &[u8]) -> Option<KeyCode> {
		self.raw.get(scancode)
	}

	pub fn modified(&self, raw: KeyCode, modifiers: Modifiers) -> Option<KeyCode> {
		use {KeyCode::*, SpecialKeyCode::*};
		let k = match (modifiers.altgr, modifiers.caps) {
			(true, true) => self.translate_altgr_caps.get(&raw).copied(),
			(true, false) => self.translate_altgr.get(&raw).copied(),
			(false, true) => match raw {
				Unicode(c @ 'a'..='z') => Some(Unicode(c.to_ascii_uppercase())),
				_ => self.translate_caps.get(&raw).copied(),
			},
			(false, false) => None,
		};
		k.or_else(|| {
			Some(match raw {
				Special(KeypadSlash) => Unicode('/'),
				Special(KeypadStar) => Unicode('*'),
				Special(KeypadMinus) => Unicode('-'),
				Special(KeypadPlus) => Unicode('+'),
				Special(KeypadEnter) => Unicode('\n'),
				k => k,
			})
		})
	}
}

pub fn parse(cfg: &[u8]) -> Result<Config, Error<'_>> {
	use scf::Token;

	let mut it = scf::parse(cfg);

	let mut cfg = Config::default();

	let assert_tk = |tk, eq| (tk == eq).then(|| ()).ok_or(Error::UnexpectedToken);
	let get_str = |tk| {
		if let Token::Str(s) = tk {
			Ok(s)
		} else {
			Err(Error::UnexpectedToken)
		}
	};
	let mut next = || it.next().transpose().map_err(Error::Syntax);

	while let Some(tk) = next()? {
		let mut next = || next()?.ok_or(Error::ExpectedToken);
		assert_tk(tk, Token::Begin)?;
		match get_str(next()?)? {
			b"raw" => loop {
				match next()? {
					Token::Begin => {}
					Token::End => break,
					Token::Str(_) => Err(Error::UnexpectedToken)?,
				}
				let keycode = parse_keycode(get_str(next()?)?)?;
				let mut buf = [0; 16];
				let mut i = 0;
				loop {
					match next()? {
						Token::Begin => Err(Error::UnexpectedToken)?,
						Token::End => break,
						Token::Str(s) => {
							*buf.get_mut(i).ok_or(Error::ScancodeTooLong)? = parse_hex_u8(s)?;
							i += 1;
						}
					}
				}
				let prev = match &buf[..i] {
					&[a] => cfg.raw.map_single[usize::from(a)].replace(keycode),
					&[a, b] => cfg.raw.map_double.insert([a, b], keycode),
					s => cfg.raw.map_long.insert(s.into(), keycode),
				};
				// TODO somehow log or return *warning* if a duplicate key is found
				let _ = prev;
			},
			s @ b"caps" | s @ b"altgr" | s @ b"altgr+caps" => loop {
				match next()? {
					Token::Begin => {}
					Token::End => break,
					Token::Str(_) => Err(Error::UnexpectedToken)?,
				}
				let target = parse_keycode(get_str(next()?)?)?;
				let source = parse_keycode(get_str(next()?)?)?;
				assert_tk(next()?, Token::End)?;
				let prev = match s {
					b"caps" => cfg.translate_caps.insert(source, target),
					b"altgr" => cfg.translate_altgr.insert(source, target),
					b"altgr+caps" => cfg.translate_altgr_caps.insert(source, target),
					_ => unreachable!(),
				};
				// TODO ditto
				let _ = prev;
			},
			s => Err(Error::UnknownSection(s))?,
		}
	}

	Ok(cfg)
}

#[derive(Debug)]
pub enum Error<'a> {
	ExpectedToken,
	UnexpectedToken,
	UnknownSection(&'a [u8]),
	UnknownKeyCode(&'a [u8]),
	Syntax(scf::Error),
	InvalidByte,
	InvalidUtf8,
	ScancodeTooLong,
}

fn parse_keycode(s: &[u8]) -> Result<KeyCode, Error> {
	use KeyCode::*;
	use SpecialKeyCode::*;
	Ok(match s {
		b"backspace" => Unicode('\x08'),
		b"space" => Unicode(' '),
		b"tab" => Unicode('\t'),
		b"capslock" => Special(CapsLock),
		b"lshift" => Special(LeftShift),
		b"rshift" => Special(RightShift),
		b"lgui" => Special(LeftGui),
		b"rgui" => Special(RightGui),
		b"lctrl" => Special(LeftControl),
		b"rctrl" => Special(RightControl),
		b"alt" => Special(Alt),
		b"altgr" => Special(AltGr),
		b"menu" => Special(Menu),
		b"enter" => Unicode('\n'),
		b"esc" => Unicode('\x1b'),
		b"f0" => Special(F0),
		b"f1" => Special(F1),
		b"f2" => Special(F2),
		b"f3" => Special(F3),
		b"f4" => Special(F4),
		b"f5" => Special(F5),
		b"f6" => Special(F6),
		b"f7" => Special(F7),
		b"f8" => Special(F8),
		b"f9" => Special(F9),
		b"f10" => Special(F10),
		b"f11" => Special(F11),
		b"f12" => Special(F12),
		b"f13" => Special(F13),
		b"f14" => Special(F14),
		b"f15" => Special(F15),
		b"f16" => Special(F16),
		b"f17" => Special(F17),
		b"f18" => Special(F18),
		b"f19" => Special(F19),
		b"f20" => Special(F20),
		b"f21" => Special(F21),
		b"f22" => Special(F22),
		b"f23" => Special(F23),
		b"f24" => Special(F24),
		b"printscreen" => Special(PrintScreen),
		b"scrollock" => Special(ScrollLock),
		b"insert" => Special(Insert),
		b"home" => Special(Home),
		b"pageup" => Special(PageUp),
		b"pagedown" => Special(PageDown),
		b"delete" => Unicode('\x7f'),
		b"end" => Special(End),
		b"up" => Special(UpArrow),
		b"left" => Special(LeftArrow),
		b"down" => Special(DownArrow),
		b"right" => Special(RightArrow),
		b"kpnumlock" => Special(KeypadNumLock),
		b"kp/" => Special(KeypadSlash),
		b"kp*" => Special(KeypadStar),
		b"kp-" => Special(KeypadMinus),
		b"kp+" => Special(KeypadPlus),
		b"kpenter" => Special(KeypadEnter),
		b"kp." => Special(KeypadDot),
		b"kp0" => Special(KeypadN0),
		b"kp1" => Special(KeypadN1),
		b"kp2" => Special(KeypadN2),
		b"kp3" => Special(KeypadN3),
		b"kp4" => Special(KeypadN4),
		b"kp5" => Special(KeypadN5),
		b"kp6" => Special(KeypadN6),
		b"kp7" => Special(KeypadN7),
		b"kp8" => Special(KeypadN8),
		b"kp9" => Special(KeypadN9),
		b"pause" => Special(Pause),
		b => {
			let mut s = str::from_utf8(b).map_err(|_| Error::InvalidUtf8)?.chars();
			let c = s.next().ok_or(Error::UnknownKeyCode(b))?;
			if !s.next().is_none() {
				Err(Error::UnknownKeyCode(b))?;
			}
			Unicode(c)
		}
	})
}

fn parse_hex_u8(s: &[u8]) -> Result<u8, Error> {
	let f = |c| {
		Ok(match c {
			b'0'..=b'9' => c - b'0',
			b'a'..=b'f' => c - b'a' + 10,
			b'A'..=b'F' => c - b'A' + 10,
			_ => return Err(Error::InvalidByte),
		})
	};
	match s {
		&[a] => f(a),
		&[a, b] => Ok(f(a)? << 4 | f(b)?),
		_ => Err(Error::InvalidByte),
	}
}
