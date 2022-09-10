use {
	scancodes::{
		KeyCode::{self, *},
		SpecialKeyCode::*,
	},
	usb_hid_item::MainFlags,
	usb_hid_usage::{button, generic_desktop, Usage},
};

/// Translate HID usage IDs to keycodes
pub fn hid_to_keycode(usage: (u16, u16), flags: MainFlags) -> Option<KeyCode> {
	Some(match Usage::try_from(usage).ok()? {
		Usage::GenericDesktop(u) => match u {
			generic_desktop::Usage::X => Special(if flags.relative() { MouseX } else { AbsoluteX }),
			generic_desktop::Usage::Y => Special(if flags.relative() { MouseY } else { AbsoluteY }),
			_ => return None,
		},
		Usage::Button(u) => match u {
			button::Usage::NoButton => return None,
			button::Usage::Button(n) => Special(
				*[
					Mouse0, Mouse1, Mouse2, Mouse3, Mouse4, Mouse5, Mouse6, Mouse7, Mouse8, Mouse9,
					Mouse10, Mouse11, Mouse12, Mouse13, Mouse14, Mouse15,
				]
				.get(usize::from(n.get() - 1))?,
			),
			_ => return None,
		},
		_ => return None,
	})
}
