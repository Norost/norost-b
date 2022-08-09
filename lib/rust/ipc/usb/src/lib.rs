#![no_std]

pub const PACKET_TY_REQUEST: u8 = 0;

#[repr(C)]
pub struct Request {
	pub ty: u8,
	pub request: u8,
	pub value: u16,
	pub index: u16,
	pub length: u16,
}

impl Request {
	pub const DIRECTION_HOST_TO_DEV: u8 = 0 << 7;
	pub const DIRECTION_DEV_TO_HOST: u8 = 1 << 7;

	pub const REQUEST_TYPE_STANDARD: u8 = 0 << 5;
	pub const REQUEST_TYPE_CLASS: u8 = 1 << 5;
	pub const REQUEST_TYPE_VENDOR: u8 = 2 << 5;

	pub const RECIPIENT_DEVICE: u8 = 0;
	pub const RECIPIENT_INTERFACE: u8 = 1;
	pub const RECIPIENT_ENDPOINT: u8 = 2;
	pub const RECIPIENT_OTHER: u8 = 3;

	pub const STANDARD_GET_STATUS: u8 = 0;
	pub const STANDARD_CLEAR_FEATURE: u8 = 1;
	pub const STANDARD_SET_FEATURE: u8 = 3;
	pub const STANDARD_SET_ADDRESS: u8 = 5;
	pub const STANDARD_GET_DESCRIPTOR: u8 = 6;
	pub const STANDARD_SET_DESCRIPTOR: u8 = 7;
	pub const STANDARD_GET_CONFIGURATION: u8 = 8;
	pub const STANDARD_SET_CONFIGURATION: u8 = 9;
	pub const STANDARD_GET_INTERFACE: u8 = 10;
	pub const STANDARD_SET_INTERFACE: u8 = 11;
	pub const STANDARD_SYNC_FRAME: u8 = 12;

	pub fn to_raw(&self, buf: &mut [u8]) -> usize {
		assert!(buf.len() >= 9, "buffer too small");
		buf[0] = PACKET_TY_REQUEST;
		buf[1] = self.ty;
		buf[2] = self.request;
		buf[3..=4].copy_from_slice(&self.value.to_le_bytes());
		buf[5..=6].copy_from_slice(&self.value.to_le_bytes());
		buf[7..=8].copy_from_slice(&self.value.to_le_bytes());
		9
	}
}
