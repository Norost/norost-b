use crate::{control::Control, PanicFrom};

reg! {
	SPllControl @ 0x46020
	enable set_enable [31] bool
	reference set_reference [(try 29:28)] SPllReference
	frequency set_frequency [(try 27:26)] Frequency
}

bit2enum! {
	try SPllReference
	MuxedSsc 0b01
}

reg! {
	WrPllControl
	enable set_enable [31] bool
	reference set_reference [(try 29:28)] WrPllReference
	feedback_divider set_feedback_divider [(23:16)] Fraction71
	post_divider set_post_divider [(13:8)] u8 // FIXME u6
	reference_divider set_reference_divider [(7:0)] u8
}

bit2enum! {
	try WrPllReference
	PchSsc 0b01
	MuxedSsc 0b10
	LcPll2700 0b11
}

bit2enum! {
	try Frequency
	MHz810 0b00
	MHz1350 0b01
}

/// A fractional number where the upper 7 bits are the integer part and the lower bit is
/// the fractional part.
pub struct Fraction71(u8);

impl PanicFrom<u32> for Fraction71 {
	fn panic_from(n: u32) -> Self {
		assert_eq!(n & !0xff, 0);
		Self(n as u8)
	}
}

impl From<Fraction71> for u32 {
	fn from(f: Fraction71) -> Self {
		f.0.into()
	}
}

#[derive(Clone, Copy, Debug)]
pub enum WrPll {
	N1,
	N2,
}

impl WrPll {
	fn reg(&self) -> u32 {
		match self {
			Self::N1 => 0x46040,
			Self::N2 => 0x46060,
		}
	}

	unsafe fn disable(&self, control: &mut Control) {
		let mut v = WrPllControl(control.load(self.reg()));
		v.set_enable(false);
		control.store(self.reg(), v.0);
	}
}

pub unsafe fn configure(control: &mut Control, wrpll: WrPll) {}

pub unsafe fn disable_all(control: &mut Control) {
	WrPll::N1.disable(control);
	WrPll::N2.disable(control);
	let mut v = SPllControl(control.load(SPllControl::REG));
	v.set_enable(false);
	control.store(SPllControl::REG, v.0);
}

pub fn compute_sdvo(pixel_clock: u32) {}

pub fn find_params() {}
