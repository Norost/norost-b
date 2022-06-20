use super::*;

#[allow(dead_code)]
#[repr(C)]
pub struct DetachBacking {
	header: ControlHeader,
	resource_id: u32le,
	_padding: u32le,
}
