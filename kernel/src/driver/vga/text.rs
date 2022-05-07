use crate::memory::r#virtual::phys_to_virt;
use core::fmt;

pub struct Text {
	row: u8,
	column: u8,
	colors: u8,
	lines: [[u8; Self::WIDTH as usize]; Self::HEIGHT as usize],
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
		match b {
			b'\n' => {
				self.column = 0;
				self.row += 1;
			}
			b'\r' => {
				self.column = 0;
			}
			b => {
				// SAFETY: x and y are in range
				self.lines[usize::from(self.row)][usize::from(self.column)] = b;
				unsafe {
					Self::write_byte(b, self.colors, self.column, self.row);
				}
				self.column += 1;
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
