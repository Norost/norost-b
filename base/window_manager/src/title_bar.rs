use {
	crate::{
		config::{Config, ElemStyle},
		math::{Point, Rect, Size, Vector},
		Main,
	},
	core::slice,
	fontdue::layout::{
		CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle, VerticalAlign,
	},
};

/// Split the region in two halves, the first for the title bar and the second
/// for the application.
pub fn split(config: &Config, rect: Rect) -> (Rect, Rect) {
	let tbh = u32::from(config.title_bar.height);
	let mid = rect.low().y + tbh;
	let m1 = Point::new(rect.high().x, mid);
	let m2 = Point::new(rect.low().x, mid);
	(
		Rect::from_points(rect.low(), m1),
		Rect::from_points(m2, rect.high()),
	)
}

/// Render the title bar in the given region.
pub fn render(main: &mut Main, config: &Config, rect: Rect, text: &str) {
	let color = match &config.title_bar.style {
		ElemStyle::Color(c) => *c,
	};

	main.fill(rect, color);

	let c = &config.title_bar.close;
	let m = &config.title_bar.maximize;
	let (w, h) = (c.width().max(m.width()), c.height().max(m.height()));

	let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
	layout.reset(&LayoutSettings {
		horizontal_align: HorizontalAlign::Center,
		vertical_align: VerticalAlign::Middle,
		max_width: Some(rect.size().x as _),
		max_height: Some(rect.size().y as _),
		..Default::default()
	});
	layout.append(
		slice::from_ref(&config.font),
		&TextStyle { text, font_index: 0, px: 16., user_data: () },
	);
	for g in layout.glyphs().iter().filter(|g| g.char_data.rasterize()) {
		let pos = Point::new(g.x as u32, g.y as u32);
		let size = Size::new(g.width as u32, g.height as u32);
		let (_, bm) = config.font.rasterize_config(g.key);
		let bm = bm
			.iter()
			.flat_map(|&p| {
				let (p, q) = (u32::from(p), u32::from(255 - p));
				let f = |i| ((255 * p + u32::from(color[i]) * q) / 255) as u8;
				[f(0), f(1), f(2)]
			})
			.collect::<Vec<_>>();
		let r = Rect::from_size(pos, size);
		let r = rect.calc_global_pos(r).unwrap();
		main.copy(&bm, r);
	}

	let mut v = Vec::with_capacity(3 * usize::from(w) * usize::from(h));
	let mut f = |btn: &gui3d::Texture, offset: i32| {
		v.clear();
		for c in btn.as_raw().chunks_exact(4) {
			let a = u32::from(c[3]);
			for (&x, &y) in c[..3].iter().zip(&color) {
				v.push(((u32::from(x) * a + u32::from(y) * (255 - a)) / 255) as _);
			}
		}
		let h = u32::from(btn.height());
		let d = (rect.size().y - h) / 2;
		let pos = rect.high() - Vector::ONE * (d + h) - Vector::new(offset, 0);
		let rect = Rect::from_size(pos, Size::new(16, 16));
		main.copy(&v, rect);
	};
	f(c, 0);
	f(m, i32::from(c.width()) + 4);
}
