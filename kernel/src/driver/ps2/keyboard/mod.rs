use super::*;
use crate::{object_table::Root, sync::SpinLock, wrap_idt};

enum KeyboardCommand {
	SetLed = 0xed,
	Echo = 0xee,
	GetSetScanCodeSet = 0xf0,
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
enum ScanCode {
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

#[derive(Clone, Copy, Debug)]
enum Event {
	Release(ScanCode),
	Press(ScanCode),
}

impl const Default for Event {
	fn default() -> Self {
		Self::Release(Default::default())
	}
}

static mut PORT: Port = Port::P1;

struct LossyRingBuffer {
	push: u8,
	pop: u8,
	data: [Event; 128],
}

impl LossyRingBuffer {
	fn push(&mut self, item: Event) {
		self.data[usize::from(self.push & 0x7f)] = item;
		let np = self.push.wrapping_add(1);
		if np ^ 128 != self.pop {
			self.push = np;
		}
	}

	fn pop(&mut self) -> Option<Event> {
		(self.pop != self.push).then(|| {
			let item = self.data[usize::from(self.pop & 0x7f)];
			self.pop = self.pop.wrapping_add(1);
			item
		})
	}
}

static EVENTS: SpinLock<LossyRingBuffer> = SpinLock::new(LossyRingBuffer {
	push: 0,
	pop: 0,
	data: [Default::default(); 128],
});

pub(super) unsafe fn init(port: Port, root: &Root) {
	unsafe {
		// Use scancode set 2 since it's the only set that should be supported on all systems.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
		write_raw_port_command(port, 2).unwrap();
		read_port_data_with_acknowledge().unwrap();

		// Just for sanity, ensure scancode set 2 is actually being used.
		write_raw_port_command(port, KeyboardCommand::GetSetScanCodeSet as u8).unwrap();
		write_raw_port_command(port, 0).unwrap();
		read_port_data_with_acknowledge().unwrap();
		assert_eq!(
			read_port_data_with_resend(),
			Ok(2),
			"scancode set 2 is not supported"
		);

		// Save port
		PORT = port;

		// Install an IRQ
		install_irq(port, wrap_idt!(int handle_irq));

		// Enable scanning
		write_port_command(port, PortCommand::EnableScanning).unwrap();
		read_port_data_with_acknowledge().unwrap();
	}
}

extern "C" fn handle_irq() {
	static mut BUF: [u8; 8] = [0; 8];
	static mut INDEX: u8 = 0;

	let Ok(b) = (unsafe { read_port_data_nowait() }) else {
		// TODO for some reason the keyboard fires an IRQ for seemingly no reason. Just
		// ignore them for now.
		crate::driver::apic::local_apic::get().eoi.set(0);
		return;
	};
	// SAFETY: the IRQ handler cannot be interrupt nor won't it run from multiple threads.
	unsafe {
		BUF[usize::from(INDEX)] = b;
		INDEX += 1;
	}
	crate::driver::apic::local_apic::get().eoi.set(0);

	// Try to decode the scancode. We're using scancode set 2.
	use Event::*;
	use ScanCode::*;

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
			match unsafe { &BUF[..INDEX.into()] } {
				&[] => unreachable!(),
				&[0xe0] | &[0xf0] | &[0xe0, 0xf0] => return,
				$(
					&[$($press,)*] => Press($scancode),
					scanset2_arm_release![$($press)*] => Release($scancode),
				)*
				// Pause has no release
				&[0xe1] => return,
				&[0xe1, 0x14] => return,
				&[0xe1, 0x14, 0x77] => return,
				&[0xe1, 0x14, 0x77, 0xe1] => return,
				&[0xe1, 0x14, 0x77, 0xe1, 0xf0] => return,
				&[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14] => return,
				&[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0] => return,
				&[0xe1, 0x14, 0x77, 0xe1, 0xf0, 0x14, 0xf0, 0x77] => Press(Pause),
				seq => {
					warn!("unknown scancode {:x?}", seq);
					unsafe { INDEX = 0 };
					return;
				}
			}
		};
	}

	let code = scanset2_reverse_table! {
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
	};

	dbg!(code);
	dbg!(unsafe { INDEX });

	unsafe { INDEX = 0 };

	EVENTS.isr_lock().push(code);
}
