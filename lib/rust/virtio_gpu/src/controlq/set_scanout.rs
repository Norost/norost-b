use super::*;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SetScanout {
	header: ControlHeader,
	rect: Rect,
	scanout_id: u32le,
	resource_id: u32le,
	_padding: u32le,
}

impl SetScanout {
	pub fn new(scanout_id: u32, resource_id: u32, rect: Rect, fence: Option<u64>) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_SET_SCANOUT, fence),
			rect,
			scanout_id: scanout_id.into(),
			resource_id: resource_id.into(),
			_padding: 0.into(),
		}
	}
}
