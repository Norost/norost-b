//! Standard keyboard scancodes & scansets.

#![no_std]
#![feature(const_convert, const_trait_impl, const_try)]
#![feature(variant_count)]

pub mod scanset;

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum ScanCode {
	A,
	B,
	C,
	D,
	E,
	F,
	G,
	H,
	I,
	J,
	K,
	L,
	M,
	N,
	O,
	P,
	Q,
	R,
	S,
	T,
	U,
	V,
	W,
	X,
	Y,
	Z,
	N0,
	N1,
	N2,
	N3,
	N4,
	N5,
	N6,
	N7,
	N8,
	N9,
	F0,
	F1,
	F2,
	F3,
	F4,
	F5,
	F6,
	F7,
	F8,
	F9,
	F10,
	F11,
	F12,
	Escape,
	Minus,
	Equal,
	Backspace,
	Tab,
	Enter,
	LeftControl,
	RightControl,
	BackTick,
	ForwardTick,
	BackSlash,
	ForwardSlash,
	OpenSquareBracket,
	CloseSquareBracket,
	OpenRoundBracket,
	CloseRoundBracket,
	OpenAngleBracket,
	CloseAngleBracket,
	OpenCurlyBracket,
	CloseCurlyBracket,
	LeftAlt,
	RightAlt,
	CapsLock,
	NumberLock,
	ScrollLock,
	LeftShift,
	RightShift,
	LeftGui,
	RightGui,
	SingleQuote,
	DoubleQuote,
	Dot,
	Comma,
	Colon,
	Semicolon,
	Space,
	PrintScreen,
	Pause,
	Insert,
	Delete,
	Home,
	End,
	Apps,
	PageUp,
	PageDown,
	UpArrow,
	DownArrow,
	LeftArrow,
	RightArrow,
	KeypadN0,
	KeypadN1,
	KeypadN2,
	KeypadN3,
	KeypadN4,
	KeypadN5,
	KeypadN6,
	KeypadN7,
	KeypadN8,
	KeypadN9,
	KeypadDivide,
	KeypadEnter,
	KeypadStar,
	KeypadPlus,
	KeypadMinus,
	KeypadDot,
}

impl ScanCode {
	/// Convert a scancode to an alphabet character.
	pub fn alphabet_to_char(self) -> Option<char> {
		Some(match self {
			Self::A => 'a',
			Self::B => 'b',
			Self::C => 'c',
			Self::D => 'd',
			Self::E => 'e',
			Self::F => 'f',
			Self::G => 'g',
			Self::H => 'h',
			Self::I => 'i',
			Self::J => 'j',
			Self::K => 'k',
			Self::L => 'l',
			Self::M => 'm',
			Self::N => 'n',
			Self::O => 'o',
			Self::P => 'p',
			Self::Q => 'q',
			Self::R => 'r',
			Self::S => 's',
			Self::T => 't',
			Self::U => 'u',
			Self::V => 'v',
			Self::W => 'w',
			Self::X => 'x',
			Self::Y => 'y',
			Self::Z => 'z',
			_ => return None,
		})
	}

	/// Convert a scancode to a bracket character.
	pub fn bracket_to_char(self) -> Option<char> {
		Some(match self {
			Self::OpenRoundBracket => '(',
			Self::OpenSquareBracket => '[',
			Self::OpenAngleBracket => '<',
			Self::OpenCurlyBracket => '{',
			Self::CloseRoundBracket => ')',
			Self::CloseSquareBracket => ']',
			Self::CloseAngleBracket => '>',
			Self::CloseCurlyBracket => '}',
			_ => return None,
		})
	}

	/// Convert a scancode to a number character
	pub fn number_to_char(self) -> Option<char> {
		Some(match self {
			Self::N0 | Self::KeypadN0 => '0',
			Self::N1 | Self::KeypadN1 => '1',
			Self::N2 | Self::KeypadN2 => '2',
			Self::N3 | Self::KeypadN3 => '3',
			Self::N4 | Self::KeypadN4 => '4',
			Self::N5 | Self::KeypadN5 => '5',
			Self::N6 | Self::KeypadN6 => '6',
			Self::N7 | Self::KeypadN7 => '7',
			Self::N8 | Self::KeypadN8 => '8',
			Self::N9 | Self::KeypadN9 => '9',
			_ => return None,
		})
	}

	/// Whether the scancode cooresponds to a key normally located on a numpad.
	pub fn is_keypad(self) -> bool {
		match self {
			Self::KeypadN0
			| Self::KeypadN1
			| Self::KeypadN2
			| Self::KeypadN3
			| Self::KeypadN4
			| Self::KeypadN5
			| Self::KeypadN6
			| Self::KeypadN7
			| Self::KeypadN8
			| Self::KeypadN9
			| Self::KeypadDot
			| Self::KeypadStar
			| Self::KeypadMinus
			| Self::KeypadPlus
			| Self::KeypadDivide
			| Self::KeypadEnter => true,
			_ => false,
		}
	}
}

impl const Default for ScanCode {
	fn default() -> Self {
		Self::A
	}
}

impl const From<ScanCode> for u32 {
	fn from(code: ScanCode) -> u32 {
		code as u32
	}
}

impl const From<ScanCode> for [u8; 4] {
	fn from(code: ScanCode) -> [u8; 4] {
		u32::from(code).to_le_bytes()
	}
}

#[derive(Debug)]
pub struct InvalidScanCode;

impl const TryFrom<u32> for ScanCode {
	type Error = InvalidScanCode;

	fn try_from(n: u32) -> Result<Self, Self::Error> {
		// FIXME I can't be arsed right now.
		if (n as usize) < core::mem::variant_count::<Self>() {
			unsafe { core::mem::transmute(n as u8) }
		} else {
			Err(InvalidScanCode)
		}
	}
}

impl const TryFrom<[u8; 4]> for ScanCode {
	type Error = InvalidScanCode;

	fn try_from(n: [u8; 4]) -> Result<Self, Self::Error> {
		u32::from_le_bytes(n).try_into()
	}
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
	Release(ScanCode),
	Press(ScanCode),
}

impl const Default for Event {
	fn default() -> Self {
		Self::Release(Default::default())
	}
}

impl const From<Event> for u32 {
	fn from(evt: Event) -> u32 {
		match evt {
			Event::Release(code) => (0 << 31) | u32::from(code),
			Event::Press(code) => (1 << 31) | u32::from(code),
		}
	}
}

impl const From<Event> for [u8; 4] {
	fn from(evt: Event) -> [u8; 4] {
		u32::from(evt).to_le_bytes()
	}
}

impl const TryFrom<u32> for Event {
	type Error = InvalidScanCode;

	fn try_from(n: u32) -> Result<Self, Self::Error> {
		Ok(if n & (1 << 31) == 0 {
			Event::Release(ScanCode::try_from(n)?)
		} else {
			Event::Press(ScanCode::try_from(n & !(1 << 31))?)
		})
	}
}

impl const TryFrom<[u8; 4]> for Event {
	type Error = InvalidScanCode;

	fn try_from(n: [u8; 4]) -> Result<Self, Self::Error> {
		u32::from_le_bytes(n).try_into()
	}
}
