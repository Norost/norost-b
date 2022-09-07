use core::fmt;

pub const HID: u8 = 0x21;
pub const REPORT: u8 = 0x22;
pub const PHYSICAL: u8 = 0x23;
// 0x24 - 0x2f reserved

pub struct Hid {
	pub hid_version: u16,
	pub country_code: u8,
	pub num_descriptors: u8,
	pub ty: u8,
	pub len: u16,
}

impl fmt::Debug for Hid {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let [maj, min] = self.hid_version.to_be_bytes();
		f.debug_struct(stringify!(Hid))
			.field("hid_version", &format_args!("{:x}.{:x}", maj, min))
			.field("country_code", &self.country_code)
			.field("num_descriptors", &self.num_descriptors)
			.field("ty", &format_args!("{:#04x}", self.ty))
			.field("len", &self.len)
			.finish()
	}
}

pub fn decode_hid(buf: &[u8]) -> Hid {
	if let &[a, b, c, d, e, f, g] = buf {
		Hid {
			hid_version: u16::from_le_bytes([a, b]),
			country_code: c,
			num_descriptors: d,
			ty: e,
			len: u16::from_le_bytes([f, g]),
		}
	} else {
		panic!("unexpected HID descriptor length");
	}
}
