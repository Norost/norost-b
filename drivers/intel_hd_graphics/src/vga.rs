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

pub unsafe fn disable_vga(control: &mut Control) {
	let mut v = VgaControl(control.load(VgaControl::REG));
	v.set_disable(true);
	v.set_border(false);
	control.store(VgaControl::REG, v.0);
}
