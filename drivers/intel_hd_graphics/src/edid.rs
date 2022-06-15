//! https://en.wikipedia.org/wiki/Extended_Display_Identification_Data

pub struct Edid([u8; 128]);

macro_rules! edid {
	(u8 $offset:literal $name:ident) => {
		pub fn $name(&self) -> u8 {
			self.0[$offset]
		}
	};
}

// TODO add accessors for *all* fields
impl Edid {
	pub fn new(buf: [u8; 128]) -> Result<Self, ParseEdidError> {
		if &buf[..8] != &[0, 255, 255, 255, 255, 255, 255, 0] {
			Err(ParseEdidError::BadMagic)
		} else if buf.iter().copied().map(u16::from).sum::<u16>() % 256 != 0 {
			Err(ParseEdidError::BadChecksum)
		} else {
			Ok(Self(buf))
		}
	}

	pub fn vendor_id(&self) -> u16 {
		u16::from_be_bytes(self.0[0..2].try_into().unwrap())
	}

	pub fn product_id(&self) -> u16 {
		u16::from_le_bytes(self.0[2..4].try_into().unwrap())
	}

	pub fn serial_number(&self) -> u32 {
		u32::from_le_bytes(self.0[4..8].try_into().unwrap())
	}

	edid!(u8 16 manufacture_week);
	edid!(u8 17 manufacture_year);
	edid!(u8 18 edid_version);
	edid!(u8 19 edid_revision);

	edid!(u8 21 horizontal_screen_size_cm);
	edid!(u8 22 vertical_screen_size_cm);
	edid!(u8 23 gamma);

	// TODO u2
	pub fn detailed_timing(&self, i: usize) -> Timing {
		assert!(i < 4, "invalid timing descriptor");
		let d = &self.0[54 + i * 18..][..18];
		let f12 = |l, h, shift| u16::from(d[l]) | ((u16::from(d[h]) >> shift * 4) & 0xfu16) << 8;
		let f10 = |l, h, shift| u16::from(d[l]) | ((u16::from(d[h]) >> shift * 2) & 0x3u16) << 8;
		let f6 = |l, ls, h, hs| {
			((u16::from(d[l]) >> ls * 4) & 0xfu16) | ((u16::from(d[h]) >> hs * 2) & 0x3u16) << 4
		};
		Timing {
			pixel_clock: u16::from_le_bytes(d[0..2].try_into().unwrap()),

			horizontal_active_pixels: f12(2, 4, 1),
			horizontal_blanking_pixels: f12(3, 4, 0),
			horizontal_sync_offset: f10(8, 11, 3),
			horizontal_sync_pulse_width: f10(9, 11, 2),
			horizontal_border_pixels: d[15],
			horizontal_image_size_mm: f12(12, 14, 1),

			vertical_active_lines: f12(5, 7, 1),
			vertical_blanking_lines: f12(6, 7, 0),
			vertical_sync_offset: f6(10, 1, 11, 1),
			vertical_sync_pulse_width: f6(10, 0, 11, 0),
			vertical_border_lines: d[16],
			vertical_image_size_mm: f12(13, 14, 0),
		}
	}
}

// TODO add missing fields (features_bitmap)
pub struct Timing {
	pub pixel_clock: u16,

	pub horizontal_active_pixels: u16,    // TODO u12
	pub horizontal_blanking_pixels: u16,  // TODO u12
	pub horizontal_sync_offset: u16,      // TODO u10
	pub horizontal_sync_pulse_width: u16, // TODO u10
	pub horizontal_border_pixels: u8,
	pub horizontal_image_size_mm: u16, // TODO u12

	pub vertical_active_lines: u16,     // TODO u12
	pub vertical_blanking_lines: u16,   // TODO u12
	pub vertical_sync_offset: u16,      // TODO u10
	pub vertical_sync_pulse_width: u16, // TODO u10
	pub vertical_border_lines: u8,
	pub vertical_image_size_mm: u16, // TODO u12
}

#[derive(Debug)]
pub enum ParseEdidError {
	BadMagic,
	BadChecksum,
}
