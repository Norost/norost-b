use std::fs::File;

pub struct Config {
	pub title_bar: TitleBar,
}

pub struct TitleBar {
	pub height: u16,
	pub style: ElemStyle,
	pub close: gui3d::Texture,
	pub maximize: gui3d::Texture,
}

pub enum ElemStyle {
	Color([u8; 3]),
}

pub fn load() -> Config {
	let (close, maximize) = {
		let img = File::open("drivers/button.png").unwrap();
		let mut img = png::Decoder::new(img);
		img.set_transformations(png::Transformations::normalize_to_color8());
		img.set_ignore_text_chunk(true);
		let mut lim = png::Limits::default();
		lim.bytes = 1 << 16;
		img.set_limits(lim);
		let mut img = img.read_info().unwrap();
		let info = img.info();
		assert_eq!(info.color_type, png::ColorType::Rgba);
		let mut buf = vec![0; img.output_buffer_size()];
		img.next_frame(&mut buf).unwrap();
		let img = gui3d::NormalMap::from_raw(buf, 16, 16);

		let direction = gui3d::Vec3::new(-2.0, -3.0, 5.0).normalize();
		let close = img.apply_lighting(&gui3d::Params {
			lighting: gui3d::Lighting {
				ambient: gui3d::Rgb::new(0.2, 0., 0.),
				diffuse: gui3d::Rgb::new(0.4, 0.05, 0.05),
				specular: gui3d::Rgb::new(0.3, 0.15, 0.1),
				reflection: 5,
				direction,
				..Default::default()
			},
		});
		let maximize = img.apply_lighting(&gui3d::Params {
			lighting: gui3d::Lighting {
				ambient: gui3d::Rgb::new(0.05, 0.15, 0.),
				diffuse: gui3d::Rgb::new(0.2, 0.35, 0.05),
				specular: gui3d::Rgb::new(0.05, 0.45, 0.1),
				reflection: 5,
				direction,
				..Default::default()
			},
		});
		(close, maximize)
	};
	Config {
		title_bar: TitleBar {
			height: 16 + 4,
			style: ElemStyle::Color([20, 20, 127]),
			close,
			maximize,
		},
	}
}
