use crate::{control::Control, mode::Mode};

reg! {
	SourceSize
	horizontal_source_size set_horizontal_source_size [(28:16)] u16 // FIXME u13
	vertical_source_size set_vertical_source_size [(11:0)] u16 // FIXME u12
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
}

pub unsafe fn configure(control: &mut Control, pipe: Pipe, mode: &Mode) {
	let mut sz = SourceSize(0);
	sz.set_horizontal_source_size(mode.horizontal.active);
	sz.set_vertical_source_size(mode.vertical.active);
	pipe.store_source_size(control, sz);
}

pub unsafe fn disable(_control: &mut Control, _pipe: Pipe) {}
