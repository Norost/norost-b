#![no_std]

use core::str;

pub const SEND_TY_REQUEST: u8 = 0;
pub const SEND_TY_PUBLIC_OBJECT: u8 = 1;
pub const SEND_TY_DATA_OUT: u8 = 2;
pub const SEND_TY_DATA_IN: u8 = 3;

pub const RECV_TY_DATA_IN: u8 = 0;
pub const RECV_TY_ERROR: u8 = 1;

#[derive(Clone, Copy, Debug)]
pub enum Endpoint {
	N0,
	N1,
	N2,
	N3,
	N4,
	N5,
	N6,
	N7,
	N8,
	N9,
	N10,
	N11,
	N12,
	N13,
	N14,
	N15,
}

impl TryFrom<&[u8]> for Endpoint {
	type Error = InvalidEndpoint;

	fn try_from(s: &[u8]) -> Result<Self, Self::Error> {
		use Endpoint::*;
		Ok(match s {
			b"1" => N1,
			b"2" => N2,
			b"3" => N3,
			b"4" => N4,
			b"5" => N5,
			b"6" => N6,
			b"7" => N7,
			b"8" => N8,
			b"9" => N9,
			b"10" => N10,
			b"11" => N11,
			b"12" => N12,
			b"13" => N13,
			b"14" => N14,
			b"15" => N15,
			_ => return Err(InvalidEndpoint),
		})
	}
}

#[derive(Debug)]
pub struct InvalidEndpoint;

pub fn send_public_object<R>(f: impl FnOnce(&[u8]) -> R) -> R {
	f(&[SEND_TY_PUBLIC_OBJECT])
}

pub fn send_data_out<R>(ep: Endpoint, f: impl FnOnce(&[u8]) -> R) -> R {
	f(&[SEND_TY_DATA_OUT, ep as _])
}

pub fn send_data_in<R>(ep: Endpoint, amount: u32, f: impl FnOnce(&[u8]) -> R) -> R {
	let [a, b, c, d] = amount.to_le_bytes();
	f(&[SEND_TY_DATA_IN, ep as _, a, b, c, d])
}

pub fn recv_parse(msg: &[u8]) -> Result<Recv<'_>, &'static str> {
	let f = |i, j| msg.get(i..j).ok_or("truncated message");
	let fe = |i| msg.get(i..).ok_or("truncated message");
	let fs = |i| str::from_utf8(fe(i)?).map_err(|_| "invalid UTF-8");
	let f1 = |i| f(i, i + 1).map(|l| l[0]);
	let f4 = |i| f(i, i + 4).map(|l| u32::from_le_bytes(l[0..4].try_into().unwrap()));
	Ok(match f1(0)? {
		RECV_TY_DATA_IN => Recv::DataIn { ep: f1(1)?, data: fe(2)? },
		RECV_TY_ERROR => Recv::Error { id: f4(1)?, code: f1(4)?, message: fs(5)? },
		_ => return Err("unknown message type"),
	})
}

pub enum Recv<'a> {
	DataIn { ep: u8, data: &'a [u8] },
	Error { id: u32, code: u8, message: &'a str },
}

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
		buf[0] = SEND_TY_REQUEST;
		buf[1] = self.ty;
		buf[2] = self.request;
		buf[3..=4].copy_from_slice(&self.value.to_le_bytes());
		buf[5..=6].copy_from_slice(&self.value.to_le_bytes());
		buf[7..=8].copy_from_slice(&self.value.to_le_bytes());
		9
	}
}
