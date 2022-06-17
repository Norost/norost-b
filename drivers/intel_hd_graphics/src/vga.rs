use crate::control::Control;

reg! {
	/// # Note
	///
	/// The reserved bits must be preserved.
	VgaControl @ 0x41000
	disable set_disable [31] bool
	border set_border [26] bool
	pipe_csc set_pipe_csc [24] bool
}

const SR_INDEX: u32 = 0x3c4;
const SR_DATA: u32 = 0x3c5;

struct Sr01(u8);

impl Sr01 {
	const INDEX: u8 = 0x1;

	fn set_disabled(&mut self, value: bool) {
		self.0 &= !(1 << 5);
		self.0 |= u8::from(value) << 5;
	}
}

pub unsafe fn disable_vga(control: &mut Control) {
	// Disable VGA screen
	control.store_byte(SR_INDEX, Sr01::INDEX);
	let mut sr01 = Sr01(control.load_byte(SR_DATA));
	sr01.set_disabled(true);
	control.store_byte(SR_DATA, sr01.0);
	rt::thread::sleep(core::time::Duration::from_micros(100));

	// Disable VGA plane
	let mut v = VgaControl(control.load(VgaControl::REG));
	v.set_disable(true);
	v.set_border(false);
	control.store(VgaControl::REG, v.0);
}
