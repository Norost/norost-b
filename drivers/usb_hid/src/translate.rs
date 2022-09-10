use {
	input::{Movement, Type},
	usb_hid_item::MainFlags,
	usb_hid_usage::{button, generic_desktop, Usage},
};

/// Translate HID usage IDs to keycodes
pub fn hid_to_keycode(usage: (u16, u16), flags: MainFlags) -> Option<Type> {
	let mov = |m| {
		flags
			.relative()
			.then(|| Type::Relative(0, m))
			.unwrap_or(Type::Absolute(0, m))
	};
	Some(match Usage::try_from(usage).ok()? {
		Usage::GenericDesktop(u) => match u {
			generic_desktop::Usage::X => mov(Movement::TranslationX),
			generic_desktop::Usage::Y => mov(Movement::TranslationY),
			generic_desktop::Usage::Z => mov(Movement::TranslationZ),
			_ => return None,
		},
		Usage::Button(u) => match u {
			button::Usage::NoButton => return None,
			button::Usage::Button(n) => Type::Button(n.get() - 1),
			_ => return None,
		},
		_ => return None,
	})
}
