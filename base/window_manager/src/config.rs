use std::fs::File;

pub struct Config {
	pub title_bar: TitleBar,
	pub cursor: gui3d::Texture,
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
	let direction = gui3d::Vec3::new(-2.0, -3.0, 5.0).normalize();

	let (close, maximize) = {
		let img = load_normal_map("button.png");

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

	let cursor = {
		load_normal_map("cursor.png").apply_lighting(&gui3d::Params {
			lighting: gui3d::Lighting {
				//ambient: gui3d::Rgb::new(0.7, 0.7, 0.7),
				ambient: gui3d::Rgb::new(0.5, 0.5, 0.5),
				diffuse: gui3d::Rgb::new(0.2, 0.2, 0.2),
				specular: gui3d::Rgb::new(0.4, 0.4, 0.4),
				reflection: 10,
				direction,
				..Default::default()
			},
		})
	};

	Config {
		title_bar: TitleBar {
			height: 16 + 4,
			style: ElemStyle::Color([20, 20, 127]),
			close,
			maximize,
		},
		cursor,
	}
}

fn load_normal_map(path: &str) -> gui3d::NormalMap {
	let img = File::open(path).unwrap();
	let mut img = png::Decoder::new(img);
	img.set_transformations(png::Transformations::normalize_to_color8());
	img.set_ignore_text_chunk(true);
	let mut lim = png::Limits::default();
	lim.bytes = 1 << 20;
	img.set_limits(lim);
	let mut img = img.read_info().unwrap();
	let info = img.info();
	assert_eq!(info.color_type, png::ColorType::Rgba);
	let mut buf = vec![0; img.output_buffer_size()];
	let w = info.width.try_into().unwrap();
	let h = info.height.try_into().unwrap();
	img.next_frame(&mut buf).unwrap();
	gui3d::NormalMap::from_raw(buf, w, h)
}
