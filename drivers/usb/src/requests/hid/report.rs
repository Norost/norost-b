use {
	super::super::{request_type, Direction, RawRequest},
	crate::dma::Dma,
};

// NOTE: The class bit in request_type must be set!
pub const GET_REPORT: u8 = 0x1;
pub const GET_IDLE: u8 = 0x2;
pub const GET_PROTOCOL: u8 = 0x3;
// 0x4-0x8 reserved
pub const SET_REPORT: u8 = 0x9;
pub const SET_IDLE: u8 = 0xa;
pub const SET_PROTOCOL: u8 = 0xb;

#[derive(Debug)]
pub enum Request {
	GetReport { buffer: Dma<[u8]>, ty: u8, id: u8, interface: u8 },
	GetIdle,
	GetProtocol,
	SetReport,
	SetIdle,
	SetProtocol,
}

impl Request {
	pub fn into_raw(self) -> RawRequest {
		use request_type::*;
		match self {
			Self::GetReport { buffer, ty, id, interface } => RawRequest {
				request_type: DIR_IN | TYPE_CLASS | RECIPIENT_INTERFACE,
				direction: Direction::In,
				request: GET_REPORT,
				value: u16::from(ty) << 8 | u16::from(id),
				index: interface.into(),
				buffer: Some(buffer),
			},
			e => todo!("{:?}", e),
		}
	}
}
