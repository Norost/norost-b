use alloc::{boxed::Box, string::String, vec::Vec};
use core::{ptr::NonNull, slice};
use fontdue::{
	layout::{CoordinateSystem, GlyphRasterConfig, Layout, LayoutSettings, TextStyle, WrapStyle},
	Font, Metrics,
};
use hashbrown::hash_map::{Entry, HashMap};

struct Letters {
	font: Font,
	cache: HashMap<GlyphRasterConfig, Box<[u8]>>,
}

impl Letters {
	fn new(font: Font, max_height: u16) -> Self {
		Self {
			font,
			cache: Default::default(),
		}
	}

	fn get(&mut self, key: GlyphRasterConfig) -> &[u8] {
		self.cache
			.entry(key)
			.or_insert_with(|| self.font.rasterize_config(key).1.into())
	}
}

pub struct Rasterizer {
	letters: Letters,
	lines: Vec<String>,
	min_y: u32,
}

impl Rasterizer {
	pub fn new(font: Font, max_height: u16) -> Self {
		Self {
			letters: Letters::new(font, max_height),
			lines: Default::default(),
			min_y: 0,
		}
	}

	pub fn new_line(&mut self) {
		self.lines.push(Default::default());
	}

	pub fn push_char(&mut self, c: char) {
		self.lines.is_empty().then(|| self.new_line());
		self.lines.last_mut().unwrap().push(c);
	}

	pub fn pop_char(&mut self) {
		self.lines.last_mut().and_then(|l| l.pop());
	}

	pub fn clear_line(&mut self) {
		self.lines.last_mut().map(|l| l.clear());
	}

	pub fn render_all(&mut self, framebuffer: &mut FrameBuffer) {
		let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
		layout.reset(&LayoutSettings {
			wrap_style: WrapStyle::Letter,
			max_width: Some(framebuffer.width as _),
			..Default::default()
		});
		let fonts = slice::from_ref(&self.letters.font);
		for (i, l) in self.lines.iter().enumerate() {
			layout.append(fonts, &TextStyle::new(l, 20.0, 0));
			if i != self.lines.len() - 1 {
				layout.append(fonts, &TextStyle::new("\n", 20.0, 0));
			}
		}
		// layout height is *inclusive*
		self.min_y = (layout.height() as u32 + 1)
			.saturating_sub(framebuffer.height)
			.max(self.min_y);
		for g in layout.glyphs().iter().filter(|g| g.char_data.rasterize()) {
			let (x, y) = (g.x as u32, g.y as u32);
			let bm = self.letters.get(g.key);
			if let Some(y) = y.checked_sub(self.min_y) {
				framebuffer.draw_rect(x, y, g.width as _, g.height as _, |x, y| {
					let r = bm[y as usize * g.width + x as usize];
					[r, r, r]
				})
			}
		}
	}
}

pub struct FrameBuffer {
	data: NonNull<[u8; 3]>,
	width: u32,
	height: u32,
}

impl FrameBuffer {
	pub unsafe fn new(data: NonNull<[u8; 3]>, width: u32, height: u32) -> Self {
		Self {
			data,
			width,
			height,
		}
	}

	pub fn draw_rect<F>(&mut self, x: u32, y: u32, width: u32, height: u32, mut f: F)
	where
		F: FnMut(u32, u32) -> [u8; 3],
	{
		assert!(
			x < self.width
				&& y < self.height
				&& x + width <= self.width
				&& y + height <= self.height,
			"({},{}) ({},{}) outside ({},{})",
			x,
			y,
			width,
			height,
			self.width,
			self.height
		);
		for (dy, y) in (0..height).map(|dy| (dy, (y + dy) as usize)) {
			for (dx, x) in (0..width).map(|dx| (dx, (x + dx) as usize)) {
				unsafe {
					self.data
						.as_ptr()
						.add(y * self.width as usize + x)
						.write(f(dx, dy))
				}
			}
		}
	}
}

impl AsRef<[u8]> for FrameBuffer {
	fn as_ref(&self) -> &[u8] {
		unsafe {
			slice::from_raw_parts(
				self.data.as_ptr().cast(),
				self.width as usize * self.height as usize * 3,
			)
		}
	}
}

impl AsMut<[u8]> for FrameBuffer {
	fn as_mut(&mut self) -> &mut [u8] {
		unsafe {
			slice::from_raw_parts_mut(
				self.data.as_ptr().cast(),
				self.width as usize * self.height as usize * 3,
			)
		}
	}
}
