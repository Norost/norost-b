use super::*;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Flush {
	header: ControlHeader,
	rect: Rect,
	resource_id: u32le,
	_padding: u32le,
}

impl Flush {
	pub fn new(resource_id: u32, rect: Rect, fence: Option<u64>) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_RESOURCE_FLUSH, fence),
			rect,
			resource_id: resource_id.into(),
			_padding: 0.into(),
		}
	}
}
