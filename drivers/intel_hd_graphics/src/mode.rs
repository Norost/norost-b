// Pretty much verbatim from https://github.com/avdgrinten/managarm/blob/4c4478cbde21675ca31e65566f10e1846b268bd5/drivers/gfx/intel/src/main.cpp#L61

use crate::edid::Edid;

#[derive(Clone, Copy, Debug)]
pub struct Timings {
	pub active: u16,
	pub sync_start: u16,
	pub sync_end: u16,
	pub total: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Mode {
	pub pixel_clock: u32,
	pub horizontal: Timings,
	pub vertical: Timings,
}

impl Mode {
	pub fn from_edid(edid: &Edid) -> Result<Self, <u16 as TryFrom<u32>>::Error> {
		let t = edid.detailed_timing(0);
		let dt = |active, offset, width, blank| -> Result<_, <u16 as TryFrom<u32>>::Error> {
			Ok(Timings {
				active: active - 1,
				sync_start: (u32::from(active) + u32::from(offset) - 1).try_into()?,
				sync_end: (u32::from(active) + u32::from(offset) + u32::from(width) - 1)
					.try_into()?,
				total: (u32::from(active) + u32::from(blank) - 1).try_into()?,
			})
		};
		Ok(Self {
			pixel_clock: u32::from(t.pixel_clock) * 10,
			horizontal: dt(
				t.horizontal_active_pixels,
				t.horizontal_sync_offset,
				t.horizontal_sync_pulse_width,
				t.horizontal_blanking_pixels,
			)?,
			vertical: dt(
				t.vertical_active_lines,
				t.vertical_sync_offset,
				t.vertical_sync_pulse_width,
				t.vertical_blanking_lines,
			)?,
		})
	}
}
