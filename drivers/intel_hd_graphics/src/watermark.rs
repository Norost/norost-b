use crate::control::Control;

reg! {
	WmPipeA @ 0x45100
	primary_watermark set_primary_watermark [(23:16)] u8
	sprite_watermark set_sprite_watermark [(23:16)] u8
	cursor_watermark set_cursor_watermark [(5:0)] u8 // FIXME u6
}

pub unsafe fn configure(control: &mut Control) {
	// vol11 "Watermark Method 1"
	let pixrate_mhz = 140;
	let bytes_per_pix = 4;
	let memval_micros_2 = 15;
	let imm = pixrate_mhz * bytes_per_pix * memval_micros_2 / 2;
	let finl = (imm + 63) / 64 + 2;
	let mut v = WmPipeA(control.load(WmPipeA::REG));
	v.set_primary_watermark(53);
	control.store(WmPipeA::REG, v.0);
}
