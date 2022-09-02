use {super::*, core::fmt};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TransferToHost2D {
	header: ControlHeader,
	rect: Rect,
	offset: u64le,
	resource_id: u32le,
	_padding: u32le,
}

impl TransferToHost2D {
	pub fn new(resource_id: u32, offset: u64, rect: Rect, fence: Option<u64>) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_TRANSFER_TO_HOST_2D, fence),
			rect,
			offset: offset.into(),
			resource_id: resource_id.into(),
			_padding: 0.into(),
		}
	}
}

impl fmt::Debug for TransferToHost2D {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(TransferToHost2D))
			.field("header", &self.header)
			.field("rect", &self.rect)
			.field("offset", &u64::from(self.offset))
			.field("resource_id", &u32::from(self.resource_id))
			.finish()
	}
}
