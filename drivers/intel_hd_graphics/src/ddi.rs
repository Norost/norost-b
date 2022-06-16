use crate::control::Control;

reg! {
	BufferControl
	enable set_enable [31] bool
}

#[derive(Clone, Copy, Debug)]
pub enum Ddi {
	A,
	B,
	C,
	D,
	E,
}

impl Ddi {
	fn offset(&self) -> u32 {
		0x100
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
				Self::D => 3,
				Self::E => 4,
			}
	}

	impl_reg!(0x64000 BufferControl load_buf_ctl store_buf_ctl);
}

pub unsafe fn enable(control: &mut Control, ddi: Ddi) {
	let mut v = ddi.load_buf_ctl(control);
	v.set_enable(true);
	ddi.store_buf_ctl(control, v);
}

pub unsafe fn disable(control: &mut Control, ddi: Ddi) {
	let mut v = ddi.load_buf_ctl(control);
	v.set_enable(false);
	ddi.store_buf_ctl(control, v);
}
