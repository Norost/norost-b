//! Standard keyboard scancodes & scansets.

#![no_std]
#![feature(const_trait_impl)]

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
