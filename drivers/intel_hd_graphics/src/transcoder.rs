//! # Trancoder AKA pipe configuration

use crate::{
	control::Control,
	mode::{self, Mode},
};

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
	SynchronizeShift
	//sync_shift_start
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
	impl_reg!(0x60028 SynchronizeShift load_vsyncshift store_vsyncshift);
	impl_reg!(0x60400 DdiFunctionControl load_ddi_func_ctl store_ddi_func_ctl);
}

/// # Note
///
/// The transcoder must be disabled.
pub unsafe fn configure_clock(control: &mut Control, transcoder: Transcoder, ddi: Option<Ddi>) {
	// b. Configure Transcoder Clock Select to direct the Port clock to the Transcoder
	let mut clk = transcoder.load_clock_select(control);
	clk.set_clock_select(match ddi {
		None => ClockSource::None,
		Some(Ddi::B) => ClockSource::DdiB,
		Some(Ddi::C) => ClockSource::DdiC,
		Some(Ddi::D) => ClockSource::DdiD,
		Some(Ddi::E) => ClockSource::DdiE,
	});
	transcoder.store_clock_select(control, clk);

	// c. Configure and enable planes (VGA or hires). This can be done later if desired.
	// e. Enable panel fitter if needed (must be enabled for VGA)
}

pub unsafe fn configure_rest(
	control: &mut Control,
	transcoder: Transcoder,
	ddi: Option<Ddi>,
	mode: Mode,
) {
	let f =
		|total: &mut Total, blank: &mut Blank, sync: &mut Synchronize, timings: &mode::Timings| {
			total.set_total(timings.total);
			total.set_active(timings.active);
			blank.set_blank_start(timings.active);
			blank.set_blank_end(timings.total);
			sync.set_sync_start(timings.sync_start);
			sync.set_sync_end(timings.sync_end);
		};

	// f. Configure transcoder timings, M/N/TU/VC payload size, and other pipe and transcoder
	//    settings
	// FIXME don't rely on firmware configuration

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
	//let mut vsyncshift = transcoder.load_vsyncshift(control);
	f(&mut vtotal, &mut vblank, &mut vsync, &mode.vertical);
	transcoder.store_vtotal(control, vtotal);
	transcoder.store_vblank(control, vblank);
	transcoder.store_vsync(control, vsync);
	//transcoder.store_vsyncshift(control, vsyncshift);

	// g. Configure and enable TRANS_DDI_FUNC_CTL
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
											  // FIXME using any value other than B6 causes glitches. Display probably
											  // needs to be reset to change this.
											  //ddi_func.set_bits_per_color(BitsPerColor::B8); // FIXME ditto
	ddi_func.set_bits_per_color(BitsPerColor::B6); // FIXME ditto
	transcoder.store_ddi_func_ctl(control, ddi_func);

	// h. If DisplayPort multistream - Enable pipe VC payload allocation in TRANS_DDI_FUNC_CTL
	// i. If DisplayPort multistream - Wait for ACT sent status in DP_TP_STATUS and receiver DPCD
	//    (timeout after >410us)

	// j. Configure and enable TRANS_CONF
	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(true);
	transcoder.store_config(control, cfg);
}

pub unsafe fn enable(control: &mut Control, transcoder: Transcoder) {
	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(true);
	transcoder.store_config(control, cfg);
}

pub unsafe fn disable(control: &mut Control, transcoder: Transcoder) {
	// c. Disable TRANS_CONF
	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(false);
	transcoder.store_config(control, cfg);

	// d. Wait for off status in TRANS_CONF, timeout after two frame times
	while transcoder.load_config(control).state() {
		rt::thread::yield_now();
	}

	// e. If DisplayPort multistream - use AUX to program receiver VC Payload ID table to delete
	//    stream
	// f. If done with this VC payload
	//    i. Disable VC payload allocation in TRANS_DDI_FUNC_CTL
	//    ii. Wait for ACT sent status in DP_TP_STATUS and receiver DPCD+

	// g. Disable TRANS_DDI_FUNC_CTL with DDI_Select set to None
	let mut func = transcoder.load_ddi_func_ctl(control);
	func.set_enable(false);
	func.set_ddi_select(DdiSelect::None);
	transcoder.store_ddi_func_ctl(control, func);

	// h. Disable panel fitter
}

pub unsafe fn disable_clock(control: &mut Control, transcoder: Transcoder) {
	// i. Configure Transcoder Clock Select to direct no clock to the transcoder
	let mut clk = transcoder.load_clock_select(control);
	clk.set_clock_select(ClockSource::None);
	transcoder.store_clock_select(control, clk);
}

pub unsafe fn enable_only(control: &mut Control, transcoder: Transcoder) {
	let mut cfg = transcoder.load_config(control);
	cfg.set_enable(true);
	transcoder.store_config(control, cfg);

	let mut func = transcoder.load_ddi_func_ctl(control);
	func.set_enable(true);
	transcoder.store_ddi_func_ctl(control, func);
}

#[derive(Clone, Copy)]
pub struct TranscoderState {
	clock_source: ClockSource,
	htotal: u16,
	hblank: u16,
	hsync: u16,
	vtotal: u16,
	vblank: u16,
	vsync: u16,
	vsyncshift: u16,
	ddi: DdiSelect,
}

/*
unsafe fn save_state(control: &mut Control) -> TranscoderState {
	let clk = transcoder.load_clock_select(control);
	let clock_source = clk.clock_select().unwrap();

	let htotal = transcoder.load_htotal(control);
	let hblank = transcoder.load_hblank(control);
	let hsync = transcoder.load_hsync(control);

	let vtotal = transcoder.load_vtotal(control);
	let vblank = transcoder.load_vblank(control);
	let vsync = transcoder.load_vsync(control);
	let vsyncshift = transcoder.load_vsyncshift(control);

	let mut ddi_func = transcoder.load_ddi_func_ctl(control);
	let ddi = ddi_func.
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

fn restore_state(control: &mut Control, state: TranscoderState) {
}
*/

pub unsafe fn get_hv_active(control: &mut Control, transcoder: Transcoder) -> (u16, u16) {
	let htotal = transcoder.load_htotal(control);
	let vtotal = transcoder.load_vtotal(control);
	(htotal.active(), vtotal.active())
}
