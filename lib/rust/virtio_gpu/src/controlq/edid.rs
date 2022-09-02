use {crate::ControlHeader, endian::u32le};

#[allow(dead_code)]
#[repr(C)]
pub struct GetEDID {
	header: ControlHeader,
	scanout: u32le,
	_padding: u32le,
}

#[allow(dead_code)]
#[repr(C)]
pub struct EDID {
	header: ControlHeader,
	size: u32le,
	_padding: u32le,
	edid: [u8; 1024],
}
