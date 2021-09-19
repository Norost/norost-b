use core::fmt;

pub struct Text {
	row: u8,
	column: u8,
	colors: u8,
}

impl Text {
	const WIDTH: u8 = 80;
	const HEIGHT: u8 = 24;
	const BUFFER: *mut u16 = 0x0b8000 as *mut u16;

	pub const fn new() -> Self {
		Self { row: 0, column: 0, colors: 0xf }
	}

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
		let i = isize::from(Self::WIDTH) * isize::from(y) + isize::from(x);
		let v = u16::from(b) | (u16::from(colors) << 8);
		core::ptr::write_volatile(Self::BUFFER.offset(i), v);
	}

	fn put_byte(&mut self, b: u8) {
		if b == b'\n' {
			self.column = 0;
			self.row += 1;
		} else {
			// SAFETY: x and y are in range
			unsafe {
				Self::write_byte(b, self.colors, self.column, self.row);
			}
			self.column += 1;
			if self.column >= Self::WIDTH {
				self.column = 0;
				self.row += 1;
			}
		}
		if self.row >= Self::HEIGHT {
			self.clear();
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
