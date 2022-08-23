#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use ipc_usb::Recv;
use rt_default as _;
use scancodes::{Event, KeyCode, SpecialKeyCode};

/// # Boot protocol
mod boot {
	pub const LCTRL: u8 = 1 << 0;
	pub const LSHIFT: u8 = 1 << 1;
	pub const ALT: u8 = 1 << 2;
	pub const LGUI: u8 = 1 << 3;
	pub const RCTRL: u8 = 1 << 4;
	pub const RSHIFT: u8 = 1 << 5;
	pub const ALTGR: u8 = 1 << 6;
	pub const RGUI: u8 = 1 << 7;
}

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let cfg = load_keymap();

	let stdout = rt::io::stdout().unwrap();
	let stdin = rt::io::stdin().unwrap();

	let (public_in, public_out) = rt::Object::new(rt::NewObject::MessagePipe).unwrap();

	let enqueue_read =
		|| ipc_usb::send_data_in(ipc_usb::Endpoint::N1, 8, |d| stdout.write(d)).unwrap();
	for _ in 0..16 {
		enqueue_read();
	}
	ipc_usb::send_public_object(|d| stdout.write(d)).unwrap();
	stdout.share(&public_out).unwrap();

	let mut prev_state = [0; 8];
	let mut shift_level = 0;
	let mut altgr_level = 0;
	let mut capslock = false;

	loop {
		let mut buf = [0; 32];
		let len = stdin.read(&mut buf).unwrap();
		match ipc_usb::recv_parse(&buf[..len]).unwrap() {
			Recv::DataIn { ep, data } => {
				assert!(data.len() == 8, "unexpected data size");

				let send = |k| public_in.write(&u32::from(k).to_le_bytes()).unwrap();

				// Convert modifiers to keypresses
				let mod_delta = prev_state[0] ^ data[0];
				let f = |mask, key| {
					(mod_delta & mask != 0)
						.then(|| {
							let (k, d) = if data[0] & mask != 0 {
								(Event::Press(KeyCode::Special(key)), 1)
							} else {
								(Event::Release(KeyCode::Special(key)), -1)
							};
							send(k);
							d
						})
						.unwrap_or(0)
				};
				f(boot::LCTRL, SpecialKeyCode::LeftControl);
				f(boot::RCTRL, SpecialKeyCode::RightControl);
				f(boot::LGUI, SpecialKeyCode::LeftGui);
				f(boot::RGUI, SpecialKeyCode::RightGui);
				shift_level += f(boot::LSHIFT, SpecialKeyCode::LeftShift);
				shift_level += f(boot::RSHIFT, SpecialKeyCode::RightShift);
				f(boot::ALT, SpecialKeyCode::Alt);
				altgr_level += f(boot::ALTGR, SpecialKeyCode::AltGr);

				let m = scancodes::config::Modifiers {
					caps: capslock != (shift_level != 0),
					altgr: altgr_level != 0,
					num: false,
				};
				let send = |d, press| {
					if let Some(k) = cfg.raw(&[d]) {
						let k = cfg.modified(k, m).unwrap_or(k);
						send(if press {
							Event::Press(k)
						} else {
							Event::Release(k)
						});
					} else {
						rt::eprintln!("unknown scancode {}", d);
					}
				};

				// Check for keypresses
				for d in data[2..].iter().filter(|d| **d != 0) {
					if !prev_state[2..].contains(d) {
						send(*d, true);
					}
				}

				// Check for key releases
				for d in prev_state[2..].iter().filter(|d| **d != 0) {
					if !data[2..].contains(d) {
						send(*d, false);
					}
				}

				prev_state.copy_from_slice(data);
			}
		}
		enqueue_read();
	}
}

fn load_keymap() -> scancodes::config::Config {
	let f = rt::io::file_root()
		.unwrap()
		.open(b"drivers/keyboard.scf")
		.unwrap();
	let len = f
		.seek(rt::io::SeekFrom::End(0))
		.unwrap()
		.try_into()
		.unwrap();
	f.seek(rt::io::SeekFrom::Start(0)).unwrap();
	let mut buf = Vec::with_capacity(len);
	let mut offt = 0;
	while offt < len {
		offt += f
			.read_uninit(&mut buf.spare_capacity_mut()[offt..])
			.unwrap()
			.0
			.len();
	}
	unsafe { buf.set_len(len) };
	scancodes::config::parse(&buf).unwrap()
}
