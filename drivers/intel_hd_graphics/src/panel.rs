use crate::control::Control;

reg! {
	Ctrl
	enable set_enable [31] bool
}

reg! {
	WindowPosition
	xpos set_xpos [(28:16)] u16 // FIXME u13
	ypos set_ypos [(11:0)] u16 // FIXME u12
}

reg! {
	WindowSize
	xsize set_xsize [(28:16)] u16 // FIXME u13
	ysize set_ysize [(11:0)] u16 // FIXME u12
}

#[derive(Clone, Copy, Debug)]
pub enum Pipe {
	A,
	B,
	C,
}

impl Pipe {
	fn offset(&self) -> u32 {
		0x800
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
			}
	}

	impl_reg!(0x68080 Ctrl load_control store_control);
	impl_reg!(0x68070 WindowPosition load_window_position store_window_position);
	impl_reg!(0x68074 WindowSize load_window_size store_window_size);
}

pub unsafe fn enable_fitter(control: &mut Control, pipe: Pipe) {
	let mut pos = pipe.load_window_position(control);
	pos.set_xpos(0);
	pos.set_ypos(0);
	pipe.store_window_position(control, pos);

	let mut size = pipe.load_window_size(control);
	size.set_xsize(1920);
	size.set_ysize(1080);
	pipe.store_window_size(control, size);

	let mut p = pipe.load_control(control);
	p.set_enable(true);
	pipe.store_control(control, p);
}

pub unsafe fn disable_fitter(control: &mut Control, pipe: Pipe) {
	let mut p = pipe.load_control(control);
	p.set_enable(false);
	pipe.store_control(control, p);
}

pub unsafe fn disable_all_fitters(control: &mut Control) {
	[Pipe::A, Pipe::B, Pipe::C]
		.into_iter()
		.for_each(|p| disable_fitter(control, p));
}
