//! # Trancoder AKA pipe configuration

use crate::control::Control;
use crate::mode::{self, Mode};

reg! {
	Config
	enable set_enable [31] bool
	state set_state [30] bool
	interlaced_mode set_interlaced_mode [(try 22:21)] InterlacedMode
}

bit2enum! {
	try InterlacedMode
	PfPd 0
	PfId 1
	IfId 3
}

reg! {
	Blank
	blank_end set_blank_end [(28:16)] u16 // FIXME u13
	blank_start set_blank_start [(12:0)] u16 // FIXME u13
}

reg! {
	Synchronize
	sync_end set_sync_end [(28:16)] u16 // FIXME u13
	sync_start set_sync_start [(12:0)] u16 // FIXME u13
}

reg! {
	Total
	total set_total [(28:16)] u16 // FIXME u13
	// FIXME TRANS_HTOTAL has an extra bit for this.
	active set_active [(11:0)] u16 // FIXME u12
}

reg! {
	ClockSelect
	clock_select set_clock_select [(try 31:29)] ClockSource
}

reg! {
	DdiFunctionControl
	enable set_enable [31] bool
	ddi_select set_ddi_select [(try 30:28)] DdiSelect
	ddi_mode_select set_ddi_mode_select [(try 26:24)] DdiMode
	bits_per_color set_bits_per_color [(try 22:20)] BitsPerColor
	// TODO port_sync_mode_master_select, sync_polarity, dp_vc_payload_allocate,
	// dp_port_width_selection
}

bit2enum! {
	try ClockSource
	None 0b000
	DdiB 0b010
	DdiC 0b011
	DdiD 0b100
	DdiE 0b101
}

bit2enum! {
	try DdiSelect
	None 0b000
	B 0b010
	C 0b011
	D 0b100
	E 0b101
}

bit2enum! {
	try DdiMode
	Hdmi 0b000
	Dvi 0b001
	DpSst 0b010
	DpMst 0b011
	Fdi 0b100
}

bit2enum! {
	try BitsPerColor
	B8 0b000
	B10 0b001
	B6 0b010
	B12 0b011
}

#[derive(Clone, Copy)]
pub enum Ddi {
	B,
	C,
	D,
	E,
}

#[derive(Clone, Copy)]
pub enum Transcoder {
	A,
	B,
	C,
	EDP,
}

impl Transcoder {
	fn offset(&self) -> u32 {
		0x1000
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
				Self::EDP => 0xf,
			}
	}

	unsafe fn load_clock_select(&self, control: &mut Control) -> ClockSelect {
		ClockSelect(control.load(
			0x46140
				+ match self {
					Self::A | Self::EDP => 0,
					Self::B => 4,
					Self::C => 8,
				},
		))
	}

	unsafe fn store_clock_select(&self, control: &mut Control, value: ClockSelect) {
		control.store(
			0x46140
				+ match self {
					Self::A | Self::EDP => 0,
					Self::B => 4,
					Self::C => 8,
				},
			value.0,
		)
	}

	impl_reg!(0x70008 Config load_config store_config);
	impl_reg!(0x60000 Total load_htotal store_htotal);
	impl_reg!(0x60004 Blank load_hblank store_hblank);
	impl_reg!(0x60008 Synchronize load_hsync store_hsync);
	impl_reg!(0x6000c Total load_vtotal store_vtotal);
	impl_reg!(0x60010 Blank load_vblank store_vblank);
	impl_reg!(0x60014 Synchronize load_vsync store_vsync);
	impl_reg!(0x60400 DdiFunctionControl load_ddi_func_ctl store_ddi_func_ctl);
}

/// # Note
///
/// The transcoder must be disabled.
pub unsafe fn configure(
	control: &mut Control,
	transcoder: Transcoder,
	ddi: Option<Ddi>,
	mode: Mode,
) {
	// See vol11 p. 112 "Sequences for DisplayPort"
	let f =
		|total: &mut Total, blank: &mut Blank, sync: &mut Synchronize, timings: &mode::Timings| {
			total.set_total(timings.total);
			total.set_active(timings.active);
			blank.set_blank_start(timings.active);
			blank.set_blank_end(timings.total);
			sync.set_sync_start(timings.sync_start);
			sync.set_sync_end(timings.sync_end);
		};

	let mut clk = transcoder.load_clock_select(control);
	clk.set_clock_select(match ddi {
		None => ClockSource::None,
		Some(Ddi::B) => ClockSource::DdiB,
		Some(Ddi::C) => ClockSource::DdiC,
		Some(Ddi::D) => ClockSource::DdiD,
		Some(Ddi::E) => ClockSource::DdiE,
	});
	transcoder.store_clock_select(control, clk);

	let mut htotal = transcoder.load_htotal(control);
	let mut hblank = transcoder.load_hblank(control);
	let mut hsync = transcoder.load_hsync(control);
	f(&mut htotal, &mut hblank, &mut hsync, &mode.horizontal);
	transcoder.store_htotal(control, htotal);
	transcoder.store_hblank(control, hblank);
	transcoder.store_hsync(control, hsync);

	let mut vtotal = transcoder.load_vtotal(control);
	let mut vblank = transcoder.load_vblank(control);
	let mut vsync = transcoder.load_vsync(control);
	f(&mut vtotal, &mut vblank, &mut vsync, &mode.vertical);
	transcoder.store_vtotal(control, vtotal);
	transcoder.store_vblank(control, vblank);
	transcoder.store_vsync(control, vsync);

	let mut ddi_func = transcoder.load_ddi_func_ctl(control);
	ddi_func.set_enable(true);
	ddi_func.set_ddi_select(match ddi {
		None => DdiSelect::None,
		Some(Ddi::B) => DdiSelect::B,
		Some(Ddi::C) => DdiSelect::C,
		Some(Ddi::D) => DdiSelect::D,
		Some(Ddi::E) => DdiSelect::E,
	});
	ddi_func.set_ddi_mode_select(DdiMode::DpSst); // FIXME don't hardcode
	ddi_func.set_bits_per_color(BitsPerColor::B8); // FIXME ditto
	transcoder.store_ddi_func_ctl(control, ddi_func);

	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(true);
	cfg.set_interlaced_mode(InterlacedMode::PfPd);
	transcoder.store_config(control, cfg);
}

pub unsafe fn disable(control: &mut Control, transcoder: Transcoder) {
	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(false);
	transcoder.store_config(control, cfg);
	log!("tr a {:08x}", transcoder.load_config(control).0);
	//while transcoder.load_config(control).state() {
	rt::thread::yield_now();
	//}
	log!("tr b {:08x}", transcoder.load_config(control).0);

	// TODO displayport multistream

	let mut func = transcoder.load_ddi_func_ctl(control);
	func.set_enable(false);
	func.set_ddi_select(DdiSelect::None);
	transcoder.store_ddi_func_ctl(control, func);

	let mut clk = transcoder.load_clock_select(control);
	clk.set_clock_select(ClockSource::None);
	transcoder.store_clock_select(control, clk);
}
