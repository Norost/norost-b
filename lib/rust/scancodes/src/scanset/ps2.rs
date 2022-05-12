//! Decode helpers for PS/2 keyboards.

use crate::{Event, ScanCode};

#[derive(Clone, Copy, Debug)]
pub enum DecodeError {
	/// The scancode is incomplete, i.e. more bytes are expected.
	Incomplete,
	/// The scancode is not recognized. The current bytes should be discarded.
	NotRecognized,
}

/// Decode a scancode of a PS/2 keyboard with scan set 2.
pub fn scanset2_decode(scancode: &[u8]) -> Result<Event, DecodeError> {
	// Try to decode the scancode. We're using scancode set 2.
	use {Event::*, ScanCode::*};

	// https://web.archive.org/web/20170108191232/http://www.computer-engineering.org/ps2keyboard/scancodes2.html
	macro_rules! scanset2_arm_release {
		[$a:literal] => {
			&[0xf0, $a]
		};
		[$b:literal $a:literal] => {
			&[$b, 0xf0, $a]
		};
		[$d:literal $c:literal $b:literal $a:literal] => {
			&[$d, 0xf0, $c, $b, 0xf0, $a]
		};
	}
	macro_rules! scanset2_reverse_table {
		{ $($scancode:ident [$($press:literal)*])* } => {
			match scancode {
				&[] | &[0xe0] | &[0xf0] | &[0xe0, 0xf0] => Err(DecodeError::Incomplete),
				$(
					&[$($press,)*] => Ok(Press($scancode)),
					scanset2_arm_release![$($press)*] => Ok(Release($scancode)),
				)*
				// TODO Pause/break is quite odd. According to the table it is a single scancode
				// that is 8 bytes long, but you may notice that there are 0xf0 bytes in it,
				// implying a break (release) code. It also seems like it is actually composed
				// of multiple scancodes, like PrintScreen.
				&[0xe1]
				| &[0xe1, 0x14]
				| &[0xe1, 0x14, 0x77]
				| &[0xe1, 0x14, 0x77, 0xe1]
				| &[0xe1, 0x14, 0x77, 0xe1, 0xf0]
				| &[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14]
				| &[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0] => Err(DecodeError::Incomplete),
				&[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0, 0x77] => Ok(Press(Pause)),
				_ => Err(DecodeError::NotRecognized),
			}
		};
	}

	scanset2_reverse_table! {
		A [0x1c]
		B [0x32]
		C [0x21]
		D [0x23]
		E [0x24]
		F [0x2b]
		G [0x34]
		H [0x33]
		I [0x43]
		J [0x3b]
		K [0x42]
		L [0x4b]
		M [0x3a]
		N [0x31]
		O [0x44]
		P [0x4d]
		Q [0x15]
		R [0x2d]
		S [0x1b]
		T [0x2c]
		U [0x3c]
		V [0x2a]
		W [0x1d]
		X [0x22]
		Y [0x35]
		Z [0x1a]
		N0 [0x45]
		N1 [0x16]
		N2 [0x1e]
		N3 [0x26]
		N4 [0x25]
		N5 [0x2e]
		N6 [0x36]
		N7 [0x3d]
		N8 [0x3e]
		N9 [0x46]
		BackTick [0x0e]
		Minus [0x4e]
		Equal [0x55]
		BackSlash [0x5d]
		Backspace [0x66]
		Space [0x29]
		Tab [0x0d]
		CapsLock [0x58]
		LeftShift [0x12]
		RightShift [0x14]
		LeftGui [0xe0 0x1f]
		LeftAlt [0x11]
		RightShift [0x59]
		RightControl [0xe0 0x14]
		RightGui [0xe0 0x27]
		RightAlt [0xe0 0x11]
		Apps [0xe0 0x2f]
		Enter [0x5a]
		Escape [0x76]
		F1 [0x05]
		F2 [0x06]
		F3 [0x04]
		F4 [0x0c]
		F5 [0x03]
		F6 [0x0b]
		F7 [0x83]
		F8 [0x0a]
		F9 [0x01]
		F10 [0x09]
		F11 [0x78]
		F12 [0x07]
		PrintScreen [0xe0 0x12 0xe0 0x7c]
		ScrollLock [0x7e]
		OpenSquareBracket [0x54]
		Insert [0xe0 0x70]
		Home [0xe0 0x6c]
		PageUp [0xe0 0x7d]
		Delete [0xe0 0x71]
		End [0xe0 0x69]
		PageDown [0xe0 0x7a]
		UpArrow [0xe0 0x75]
		LeftArrow [0xe0 0x6b]
		DownArrow [0xe0 0x72]
		RightArrow [0xe0 0x74]
		NumberLock [0x77]
		KeypadDivide [0xe0 0x4a]
		KeypadStar [0x7c]
		KeypadMinus [0x7b]
		KeypadPlus [0x79]
		KeypadEnter [0xe0 0x5a]
		KeypadDot [0x71]
		KeypadN0 [0x70]
		KeypadN1 [0x69]
		KeypadN2 [0x72]
		KeypadN3 [0x7a]
		KeypadN4 [0x6b]
		KeypadN5 [0x73]
		KeypadN6 [0x74]
		KeypadN7 [0x6c]
		KeypadN8 [0x75]
		KeypadN9 [0x7d]
		CloseSquareBracket [0x5b]
		Semicolon [0x4c]
		SingleQuote [0x52]
		Comma [0x41]
		Dot [0x49]
		ForwardSlash [0x4a]
	}
}
