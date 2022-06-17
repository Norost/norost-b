use crate::{control::Control, GraphicsAddress};

reg! {
	PrimaryControl
	enable set_enable [31] bool
	gamma_enable set_gamma_enable [30] bool
	pixel_format set_pixel_format [(try 29:25)] PixelFormat
	pipe_csc_enable set_pipe_csc_enable [24] bool
	rotate_180 set_rotate_180 [15] bool
	// "Do not program this field to 1b" m'kay?
	//trickle_feed set_trickle_feed [14] bool
	tiled_surface set_tiled_surface [10] bool
	async_address_update_enable set_async_address_update_enable [9] bool
	stereo_surface_vblank_mask set_stereo_surface_vblank_mask [(try 7:6)] VBlankMask
}

reg! {
	PrimaryOffset
	start_y_position set_start_y_position [(27:16)] u16 // FIXME 12 bit
	start_x_position set_start_x_position [(12:0)] u16 // FIXME 13 bit
}

reg! {
	PrimaryStride
	stride set_stride [(15:6)] u16 // FIXME 10 bit
}

reg! {
	PrimarySurface
	base_address set_base_address [(31:12)] GraphicsAddress
	ring_flip_source set_ring_flip_source [3] bool
}

bit2enum! {
	try PixelFormat
	Indexed 0b0010
	BGRX565 0b0101
	BGRX8888 0b0110
	RGBX2101010 0b1000
	XrBiasRGBX2101010 0b1001
	BGRX2101010 0b1010
	RGB16161616XFp 0b1100
	RGBX8888 0b1110
}

bit2enum! {
	try VBlankMask
	None 0b00
	MaskLeft 0b01
	MaskRight 0b10
}

#[derive(Clone, Copy)]
pub enum Plane {
	A,
	B,
	C,
}

impl Plane {
	fn offset(&self) -> u32 {
		0x1000
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
			}
	}

	impl_reg!(0x70180 PrimaryControl load_primary_control store_primary_control);
	impl_reg!(0x70188 PrimaryStride load_primary_stride store_primary_stride);
	impl_reg!(0x7019c PrimarySurface load_primary_surface store_primary_surface);
	impl_reg!(0x701a4 PrimaryOffset load_primary_offset store_primary_offset);
	//impl_plane!(0x701b0 PrimaryLeftSurface load_primary_left_surface store_primary_left_surface);
}

pub struct Config {
	pub base: GraphicsAddress,
	pub format: PixelFormat,
	pub stride: u16,
}

pub unsafe fn enable(control: &mut Control, plane: Plane, config: Config) {
	// TODO make a type that guarantees stride is properly aligned.
	assert_eq!(config.stride & 63, 0, "stride must be a multiple of 64");

	// Reserved fields are all MBZ
	let mut offt = PrimaryOffset(0);
	offt.set_start_x_position(0);
	offt.set_start_y_position(0);
	plane.store_primary_offset(control, offt);

	let mut stride = plane.load_primary_stride(control);
	stride.set_stride(config.stride / 64);
	plane.store_primary_stride(control, stride);

	let mut surf = plane.load_primary_surface(control);
	surf.set_base_address(config.base);
	plane.store_primary_surface(control, surf);

	let mut ctl = plane.load_primary_control(control);
	ctl.set_enable(true);
	ctl.set_gamma_enable(false);
	ctl.set_pixel_format(config.format);
	ctl.set_pipe_csc_enable(false);
	ctl.set_rotate_180(false);
	ctl.set_tiled_surface(false);
	ctl.set_async_address_update_enable(false);
	plane.store_primary_control(control, ctl);
}

pub unsafe fn disable(control: &mut Control, plane: Plane) {
	let mut ctl = plane.load_primary_control(control);
	ctl.set_enable(false);
	plane.store_primary_control(control, ctl);
}

pub unsafe fn get_stride(control: &mut Control, plane: Plane) -> u16 {
	plane.load_primary_stride(control).stride() * 64
}
