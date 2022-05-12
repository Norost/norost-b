use crate::memory::r#virtual::phys_to_virt;
use core::{
	fmt,
	sync::atomic::{AtomicU16, Ordering},
};

#[derive(Clone, Copy)]
enum AnsiState {
	Escape,
	BracketOpen,
	Erase,
}

pub struct Text {
	row: u8,
	column: u8,
	colors: u8,
	lines: [[u8; Self::WIDTH as usize]; Self::HEIGHT as usize],
	ansi_state: Option<AnsiState>,
}

impl Text {
	const WIDTH: u8 = 80;
	const HEIGHT: u8 = 25;

	pub const fn new() -> Self {
		Self {
			row: 0,
			column: 0,
			colors: 0xf,
			lines: [[0; 80]; 25],
			ansi_state: None,
		}
	}

	#[allow(dead_code)]
	pub fn set_colors(&mut self, fg: u8, bg: u8) {
		self.colors = (fg & 0xf) | (bg << 4);
	}

	pub fn clear(&mut self) {
		for y in 0..24 {
			for x in 0..80 {
				unsafe {
					Self::write_byte(0, 0, x, y);
				}
			}
		}
		self.row = 0;
		self.column = 0;
	}

	unsafe fn write_byte(b: u8, colors: u8, x: u8, y: u8) {
		let buffer = unsafe { phys_to_virt(0xb8000).cast::<u16>() };
		let i = isize::from(Self::WIDTH) * isize::from(y) + isize::from(x);
		let v = u16::from(b) | (u16::from(colors) << 8);
		unsafe { core::ptr::write_volatile(buffer.offset(i), v) };
	}

	/// Scroll the terminal downwards once
	fn scroll_down(&mut self) {
		// Move lines up
		for y in 1..self.row {
			self.lines[usize::from(y - 1)] = self.lines[usize::from(y)];
		}

		// Clear last line
		self.row -= 1;
		self.lines[usize::from(self.row)] = [0; Self::WIDTH as usize];

		// Redraw
		for y in 0..Self::HEIGHT {
			for x in 0..Self::WIDTH {
				// SAFETY: x and y are in range
				unsafe {
					Self::write_byte(
						self.lines[usize::from(y)][usize::from(x)],
						self.colors,
						x,
						y,
					);
				}
			}
		}
	}

	fn put_byte(&mut self, b: u8) {
		let mut put = |b| {
			self.lines[usize::from(self.row)][usize::from(self.column)] = b;
			// SAFETY: x and y are in range (otherwise we'd have panicked already)
			unsafe {
				Self::write_byte(b, self.colors, self.column, self.row);
			}
			self.column += 1;
		};

		if let Some(ansi_state) = self.ansi_state {
			self.ansi_state = match (ansi_state, b) {
				(AnsiState::Escape, b'[') => Some(AnsiState::BracketOpen),
				(AnsiState::BracketOpen, b'2') => Some(AnsiState::Erase),
				(AnsiState::Erase, b'K') => {
					// Erase current line
					self.column = 0;
					self.lines[usize::from(self.row)].fill(b' ');
					for x in 0..Self::WIDTH {
						unsafe {
							Self::write_byte(b' ', self.colors, x, self.row);
						}
					}
					None
				}
				_ => {
					put(b'?');
					None
				}
			};
		} else {
			match b {
				b'\n' => {
					self.column = 0;
					self.row += 1;
				}
				b'\r' => {
					self.column = 0;
				}
				b'\x1b' => {
					self.ansi_state = Some(AnsiState::Escape);
				}
				b => put(b),
			}
		}

		if self.column >= Self::WIDTH {
			self.column = 0;
			self.row += 1;
		}

		if self.row >= Self::HEIGHT {
			self.scroll_down();
		}
	}
}

impl fmt::Write for Text {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for b in s.bytes() {
			self.put_byte(b);
		}
		Ok(())
	}
}

/// VGA text device for emergency situations. This device writes straight over any other text
/// and doesn't implement scroll. It should only be used when things are in an extremely bad state
/// (e.g. panic). It does not use a lock for synchronization, though it is still thread-safe.
pub struct EmergencyWriter;

static EMERGENCY_WRITE_POS: AtomicU16 = AtomicU16::new(0);
const EMERGENCY_COLOR: u8 = 0xc;

impl EmergencyWriter {
	fn put(c: u8) {
		EMERGENCY_WRITE_POS
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |pos| {
				let (mut row, mut col) =
					(pos / u16::from(Text::WIDTH), pos % u16::from(Text::WIDTH));
				// Clear line if the cursor is at the start of it.
				if col == 0 {
					for x in 0..Text::WIDTH {
						unsafe { Text::write_byte(b' ', EMERGENCY_COLOR, x, row as u8) };
					}
				}
				if c == b'\n' {
					row += 1;
					col = 0;
				} else {
					unsafe { Text::write_byte(c, EMERGENCY_COLOR, col as u8, row as u8) }
					col += 1;
					if col >= Text::WIDTH.into() {
						row += 1;
						col = 0;
					}
				}
				row %= u16::from(Text::HEIGHT);
				Some(row * u16::from(Text::WIDTH) + col)
			})
			.unwrap();
	}
}

impl fmt::Write for EmergencyWriter {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		Ok(s.bytes().for_each(Self::put))
	}
}
