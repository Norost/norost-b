pub mod regs {
	pub const OFFSET: u32 = 0x70184;
	pub const STRIDE: u32 = 0x70188;
	pub const ADDRESS: u32 = 0x7019c;
}

reg! {
	PlaneControl @ 0x70180
	pixel_format set_pixel_format [(try 30:26)] PixelFormat
	enable set_enable [31] bool
}

bit2enum! {
	try PixelFormat
	Indexed 2
	BGRX8888 6
	RGBX8888 14
}
