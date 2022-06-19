use super::*;

#[allow(dead_code)]
const MAX_SCANOUTS: u32 = 16;

#[allow(dead_code)]
#[repr(C)]
pub struct DisplayInfo {
	header: ControlHeader,
	pmodes: [DisplayOne; MAX_SCANOUTS as usize],
}

#[allow(dead_code)]
#[repr(C)]
pub struct DisplayOne {
	rect: Rect,
	enabled: u32le,
	flags: u32le,
}
