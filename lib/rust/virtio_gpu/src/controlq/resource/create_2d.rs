use {crate::ControlHeader, core::fmt, endian::u32le};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Create2D {
	header: ControlHeader,
	resource_id: u32le,
	format: u32le,
	width: u32le,
	height: u32le,
}

impl Create2D {
	pub fn new(
		resource_id: u32,
		format: Format,
		width: u32,
		height: u32,
		fence: Option<u64>,
	) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_RESOURCE_CREATE_2D, fence),
			resource_id: resource_id.into(),
			format: u32::from(format).into(),
			width: width.into(),
			height: height.into(),
		}
	}
}

impl fmt::Debug for Create2D {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut d = f.debug_struct("resource::Create2D");
		d.field("header", &self.header);
		d.field("resource_id", &u32::from(self.resource_id));

		match Format::try_from(u32::from(self.format)) {
			Ok(f) => d.field("format", &f),
			Err(()) => d.field("format", &format_args!("0x{:x}", u32::from(self.format))),
		};

		d.field("width", &u32::from(self.resource_id));
		d.field("height", &u32::from(self.resource_id));
		d.finish()
	}
}

#[derive(Clone, Copy, Debug)]
#[repr(u32)]
#[non_exhaustive]
pub enum Format {
	Bgra8Unorm = 1,
	Bgrx8Unorm = 2,
	Argb8Unorm = 3,
	Xrgb8Unorm = 4,
	Rgba8Unorm = 67,
	Xbgr8Unorm = 68,
	Abgr8Unorm = 121,
	Rgbx8Unorm = 134,
}

impl From<Format> for u32 {
	fn from(format: Format) -> u32 {
		format as u32
	}
}

impl TryFrom<u32> for Format {
	type Error = ();

	fn try_from(format: u32) -> Result<Self, Self::Error> {
		Ok(match format {
			1 => Self::Bgra8Unorm,
			2 => Self::Bgrx8Unorm,
			3 => Self::Argb8Unorm,
			4 => Self::Xrgb8Unorm,
			67 => Self::Rgba8Unorm,
			68 => Self::Xbgr8Unorm,
			121 => Self::Abgr8Unorm,
			134 => Self::Rgbx8Unorm,
			_ => Err(())?,
		})
	}
}
