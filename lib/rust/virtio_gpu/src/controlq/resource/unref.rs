use super::*;

#[allow(dead_code)]
#[repr(C)]
pub struct Unreference {
	header: ControlHeader,
	resource_id: u32le,
	padding: u32le,
}
