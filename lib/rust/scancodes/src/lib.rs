//! # Standard keyboard keycodes.
//!
//! ## Keycode format
//!
//! Keycodes are represented as 21-bit characters and a special bit.
//!
//! Characters equal or below `0x10ffff` directly map to their corresponding Unicode character.
//! Characters above `0x10ffff` map to the keycodes defined below:
//!
//! | Character(s)          | Keycode(s) |
//! | --------------------- | ---------- |
//! | `0x110000`-`0x110018` | F0-F24     |
//!
//! ## Event format
//!
//! An event is a 32-bit little-endian number.
//!
//! | Bit(s) | Description |
//! | ------ | ----------- |
//! | 31:24  | Modifiers   |
//! | 23:22  | Reserved    |
//! | 21     | Pressed     |
//! | 20:0   | Character   |
//!
//! ### Modifier format
//!
//! | Bit | Description |
//! | --- | ----------- |
//! | 7   | Left Ctrl   |
//! | 6   | Left Shift  |
//! | 5   | Left Alt    |
//! | 4   | Left GUI    |
//! | 3   | Right Ctrl  |
//! | 2   | Right Shift |
//! | 1   | Right Alt   |
//! | 0   | Right GUI   |

#![no_std]
#![feature(const_convert, const_trait_impl, const_try)]

#[cfg(feature = "config")]
pub mod config;

use core::fmt;

macro_rules! special_keycode {
	{ $($k:ident $v:literal)* } => {
		#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
		#[non_exhaustive]
		pub enum SpecialKeyCode { $($k,)* }

		impl From<SpecialKeyCode> for u32 {
			fn from(k: SpecialKeyCode) -> Self {
				use SpecialKeyCode::*;
				0x110000 | match k {
					$($k => $v,)*
				}
			}
		}

		#[derive(Debug)]
		pub struct InvalidSpecialKeyCode;

		impl TryFrom<u32> for SpecialKeyCode {
			type Error = InvalidSpecialKeyCode;

			fn try_from(n: u32) -> Result<Self, Self::Error> {
				use SpecialKeyCode::*;
				Ok(match n.checked_sub(0x110000).ok_or(InvalidSpecialKeyCode)? {
					$($v => $k,)*
					_ => return Err(InvalidSpecialKeyCode),
				})
			}
		}
	};
}

special_keycode! {
	F0 0x0
	F1 0x1
	F2 0x2
	F3 0x3
	F4 0x4
	F5 0x5
	F6 0x6
	F7 0x7
	F8 0x8
	F9 0x9
	F10 0xa
	F11 0xb
	F12 0xc
	F13 0xd
	F14 0xe
	F15 0xf
	F16 0x10
	F17 0x11
	F18 0x12
	F19 0x13
	F20 0x14
	F21 0x15
	F22 0x16
	F23 0x17
	F24 0x18
	PrintScreen 0x19
	ScrollLock 0x1a
	Pause 0x1b
	CapsLock 0x1c
	UpArrow 0x1d
	DownArrow 0x1e
	LeftArrow 0x1f
	RightArrow 0x20

	Insert 0x21
	Home 0x22
	End 0x23
	Menu 0x24
	PageUp 0x25
	PageDown 0x26

	LeftControl 0x30
	LeftShift 0x31
	Alt 0x32
	LeftGui 0x33
	RightControl 0x34
	RightShift 0x35
	AltGr 0x36
	RightGui 0x37

	KeypadN0 0x40
	KeypadN1 0x41
	KeypadN2 0x42
	KeypadN3 0x43
	KeypadN4 0x44
	KeypadN5 0x45
	KeypadN6 0x46
	KeypadN7 0x47
	KeypadN8 0x48
	KeypadN9 0x49
	KeypadNumLock 0x50
	KeypadSlash 0x51
	KeypadStar 0x52
	KeypadPlus 0x53
	KeypadMinus 0x54
	KeypadEnter 0x55
	KeypadDot 0x56
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCode {
	Unicode(char),
	Special(SpecialKeyCode),
}

impl fmt::Debug for KeyCode {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::Unicode(c) => c.fmt(f),
			Self::Special(c) => c.fmt(f),
		}
	}
}

impl Default for KeyCode {
	fn default() -> Self {
		Self::Unicode('\0')
	}
}

impl From<KeyCode> for u32 {
	fn from(k: KeyCode) -> Self {
		match k {
			KeyCode::Unicode(c) => c as _,
			KeyCode::Special(c) => c.into(),
		}
	}
}

#[derive(Debug)]
pub struct InvalidKeyCode;

impl TryFrom<u32> for KeyCode {
	type Error = InvalidKeyCode;

	fn try_from(n: u32) -> Result<Self, Self::Error> {
		Ok(if n < 0x110000 {
			KeyCode::Unicode(char::try_from(n).map_err(|_| InvalidKeyCode)?)
		} else {
			KeyCode::Special(SpecialKeyCode::try_from(n).map_err(|_| InvalidKeyCode)?)
		})
	}
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
	Release(KeyCode),
	Press(KeyCode),
}

impl Default for Event {
	fn default() -> Self {
		Self::Release(Default::default())
	}
}

impl From<Event> for u32 {
	fn from(evt: Event) -> u32 {
		match evt {
			Event::Release(code) => (0 << 31) | u32::from(code),
			Event::Press(code) => (1 << 31) | u32::from(code),
		}
	}
}

#[derive(Debug)]
pub struct InvalidEvent;

impl TryFrom<u32> for Event {
	type Error = InvalidEvent;

	fn try_from(n: u32) -> Result<Self, Self::Error> {
		let k = KeyCode::try_from(n & !(1 << 31)).map_err(|_| InvalidEvent)?;
		Ok(if n & (1 << 31) == 0 {
			Event::Release(k)
		} else {
			Event::Press(k)
		})
	}
}
