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
	let mut cf = scf::parse2(cfg);
	let mut cfg = Config::default();

	for item in cf.iter() {
		let mut it = item.into_group().ok_or(Error::ExpectedGroup)?;
		match it.next_str().ok_or(Error::ExpectedString)? {
			"raw" => {
				for item in it {
					let mut it = item.into_group().ok_or(Error::ExpectedGroup)?;
					let keycode = parse_keycode(it.next_str().ok_or(Error::ExpectedString)?)?;
					let mut buf = [0; 16];
					let mut i = 0;
					for item in it {
						let s = item.into_str().ok_or(Error::ExpectedString)?;
						*buf.get_mut(i).ok_or(Error::ScancodeTooLong)? = parse_hex_u8(s)?;
						i += 1;
					}
					let prev = match &buf[..i] {
						&[a] => cfg.raw.map_single[usize::from(a)].replace(keycode),
						&[a, b] => cfg.raw.map_double.insert([a, b], keycode),
						s => cfg.raw.map_long.insert(s.into(), keycode),
					};
					// TODO somehow log or return *warning* if a duplicate key is found
					let _ = prev;
				}
			}
			s @ "caps" | s @ "altgr" | s @ "altgr+caps" => {
				for item in it {
					let mut it = item.into_group().ok_or(Error::ExpectedGroup)?;
					let target = parse_keycode(it.next_str().ok_or(Error::ExpectedString)?)?;
					let source = parse_keycode(it.next_str().ok_or(Error::ExpectedString)?)?;
					let prev = match s {
						"caps" => cfg.translate_caps.insert(source, target),
						"altgr" => cfg.translate_altgr.insert(source, target),
						"altgr+caps" => cfg.translate_altgr_caps.insert(source, target),
						_ => unreachable!(),
					};
					// TODO ditto
					let _ = prev;
				}
			}
			s => Err(Error::UnknownSection(s))?,
		}
	}

	Ok(cfg)
}

#[derive(Debug)]
pub enum Error<'a> {
	ExpectedGroup,
	ExpectedString,
	UnknownSection(&'a str),
	UnknownKeyCode(&'a str),
	Syntax(scf::Error),
	InvalidByte,
	InvalidUtf8,
	ScancodeTooLong,
}

fn parse_keycode(s: &str) -> Result<KeyCode, Error> {
	use KeyCode::*;
	use SpecialKeyCode::*;
	Ok(match s {
		"backspace" => Unicode('\x08'),
		"space" => Unicode(' '),
		"tab" => Unicode('\t'),
		"capslock" => Special(CapsLock),
		"lshift" => Special(LeftShift),
		"rshift" => Special(RightShift),
		"lgui" => Special(LeftGui),
		"rgui" => Special(RightGui),
		"lctrl" => Special(LeftControl),
		"rctrl" => Special(RightControl),
		"alt" => Special(Alt),
		"altgr" => Special(AltGr),
		"menu" => Special(Menu),
		"enter" => Unicode('\n'),
		"esc" => Unicode('\x1b'),
		"f0" => Special(F0),
		"f1" => Special(F1),
		"f2" => Special(F2),
		"f3" => Special(F3),
		"f4" => Special(F4),
		"f5" => Special(F5),
		"f6" => Special(F6),
		"f7" => Special(F7),
		"f8" => Special(F8),
		"f9" => Special(F9),
		"f10" => Special(F10),
		"f11" => Special(F11),
		"f12" => Special(F12),
		"f13" => Special(F13),
		"f14" => Special(F14),
		"f15" => Special(F15),
		"f16" => Special(F16),
		"f17" => Special(F17),
		"f18" => Special(F18),
		"f19" => Special(F19),
		"f20" => Special(F20),
		"f21" => Special(F21),
		"f22" => Special(F22),
		"f23" => Special(F23),
		"f24" => Special(F24),
		"printscreen" => Special(PrintScreen),
		"scrollock" => Special(ScrollLock),
		"insert" => Special(Insert),
		"home" => Special(Home),
		"pageup" => Special(PageUp),
		"pagedown" => Special(PageDown),
		"delete" => Unicode('\x7f'),
		"end" => Special(End),
		"up" => Special(UpArrow),
		"left" => Special(LeftArrow),
		"down" => Special(DownArrow),
		"right" => Special(RightArrow),
		"kpnumlock" => Special(KeypadNumLock),
		"kp/" => Special(KeypadSlash),
		"kp*" => Special(KeypadStar),
		"kp-" => Special(KeypadMinus),
		"kp+" => Special(KeypadPlus),
		"kpenter" => Special(KeypadEnter),
		"kp." => Special(KeypadDot),
		"kp0" => Special(KeypadN0),
		"kp1" => Special(KeypadN1),
		"kp2" => Special(KeypadN2),
		"kp3" => Special(KeypadN3),
		"kp4" => Special(KeypadN4),
		"kp5" => Special(KeypadN5),
		"kp6" => Special(KeypadN6),
		"kp7" => Special(KeypadN7),
		"kp8" => Special(KeypadN8),
		"kp9" => Special(KeypadN9),
		"pause" => Special(Pause),
		b => {
			let mut s = b.chars();
			let c = s.next().ok_or(Error::UnknownKeyCode(b))?;
			if !s.next().is_none() {
				Err(Error::UnknownKeyCode(b))?;
			}
			Unicode(c)
		}
	})
}

fn parse_hex_u8(s: &str) -> Result<u8, Error> {
	let f = |c| {
		Ok(match c {
			b'0'..=b'9' => c - b'0',
			b'a'..=b'f' => c - b'a' + 10,
			b'A'..=b'F' => c - b'A' + 10,
			_ => return Err(Error::InvalidByte),
		})
	};
	match s.as_bytes() {
		&[a] => f(a),
		&[a, b] => Ok(f(a)? << 4 | f(b)?),
		_ => Err(Error::InvalidByte),
	}
}
