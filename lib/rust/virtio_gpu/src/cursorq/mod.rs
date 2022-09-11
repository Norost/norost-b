use {crate::ControlHeader, endian::u32le};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct CursorPosition {
	pub scanout_id: u32le,
	pub x: u32le,
	pub y: u32le,
	_padding: u32le,
}

impl CursorPosition {
	pub fn new(scanout_id: u32, x: u32, y: u32) -> Self {
		Self { scanout_id: scanout_id.into(), x: x.into(), y: y.into(), _padding: 0.into() }
	}
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct MoveCursor {
	header: ControlHeader,
	pub position: CursorPosition,
	pub resource_id: u32le,
	_padding: [u32le; 3],
}

impl MoveCursor {
	pub fn new(position: CursorPosition, resource_id: u32, fence: Option<u64>) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_MOVE_CURSOR, fence),
			position,
			resource_id: resource_id.into(),
			_padding: [0.into(); 3],
		}
	}
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct UpdateCursor {
	header: ControlHeader,
	pub position: CursorPosition,
	pub resource_id: u32le,
	pub hot_x: u32le,
	pub hot_y: u32le,
	_padding: u32le,
}

impl UpdateCursor {
	pub fn new(
		position: CursorPosition,
		resource_id: u32,
		hot_x: u32,
		hot_y: u32,
		fence: Option<u64>,
	) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_UPDATE_CURSOR, fence),
			position,
			resource_id: resource_id.into(),
			hot_x: hot_x.into(),
			hot_y: hot_y.into(),
			_padding: 0.into(),
		}
	}
}
