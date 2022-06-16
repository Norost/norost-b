use crate::{control::Control, mode::Mode};

reg! {
	SourceSize
	horizontal_source_size set_horizontal_source_size [(28:16)] u16 // FIXME u13
	vertical_source_size set_vertical_source_size [(11:0)] u16 // FIXME u12
}

reg! {
	Miscellaneous
	change_mask_primary_flip set_change_mask_primary_flip [23] bool
	change_mask_sprite_enable set_change_mask_sprite_enable [22] bool
	change_mask_cursor_move set_change_mask_cursor_move [21] bool
	change_mask_vblank_vsync_int set_change_mask_vblank_vsync_int [20] bool
	rotation_info set_rotation_info [(15:14)] RotationInfo
	color_space set_color_space [(11:11)] ColorSpace
	color_range_limit set_color_range_limit [10] bool
	dithering_bits_per_color set_dithering_bits_per_color [(try 7:5)] DitheringBitsPerColor
	dithering_enable set_dithering_enable [4] bool
	dithering_type set_dithering_type [(3:2)] DitheringType
	black_frame_insertion set_black_frame_insertion [0] bool
}

bit2enum! {
	RotationInfo
	None 0b00
	D90  0b01
	D180 0b10
	D270 0b11
}

bit2enum! {
	ColorSpace
	RGB 0
	YUV 1
}

bit2enum! {
	try DitheringBitsPerColor
	B8  0b000
	B10 0b001
	B6  0b010
}

bit2enum! {
	DitheringType
	Spatial 0b00
	SpatioTemporal1 0b01
	SpatioTemporal2 0b10
	Temporal 0b11
}

#[derive(Clone, Copy, Debug)]
pub enum Pipe {
	A,
	B,
	C,
}

impl Pipe {
	fn offset(&self) -> u32 {
		0x1000
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
			}
	}

	impl_reg!(0x6001c SourceSize load_source_size store_source_size);
	impl_reg!(0x70030 Miscellaneous load_miscellaneous store_miscellaneous);
}

pub unsafe fn configure(control: &mut Control, pipe: Pipe, mode: &Mode) {
	let mut sz = SourceSize(0);
	sz.set_horizontal_source_size(mode.horizontal.active);
	sz.set_vertical_source_size(mode.vertical.active);
	pipe.store_source_size(control, sz);
}

pub unsafe fn set_hv(control: &mut Control, pipe: Pipe, h: u16, v: u16) {
	let mut sz = SourceSize(0);
	sz.set_horizontal_source_size(h);
	sz.set_vertical_source_size(v);
	pipe.store_source_size(control, sz);
}

pub unsafe fn get_hv(control: &mut Control, pipe: Pipe) -> (u16, u16) {
	let sz = pipe.load_source_size(control);
	(sz.horizontal_source_size(), sz.vertical_source_size())
}
