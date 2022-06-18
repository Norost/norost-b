use crate::control::Control;

reg! {
	DpAuxCtl
	send_busy set_send_busy [31] bool
	done set_done [30] bool
	interrupt_on_done set_interrupt_on_done [29] bool
	time_out_error set_time_out_error [28] bool
	time_out_timer_value set_time_out_timer_value [(27:26)] TimerValue
	receive_error set_receive_error [25] bool
	message_size set_message_size [(try 24:20)] MessageSize
	precharge_time set_precharge_time [(19:16)] u8 // FIXME 4 bit
	bit_clock_divider set_bit_clock_divider [(try 10:0)] BitClockDivider
}

reg! {
	DpAuxData
	// MSB is transferred first
	byte_0 set_byte_0 [(31:24)] u8
	byte_1 set_byte_1 [(23:16)] u8
	byte_2 set_byte_2 [(15:8)] u8
	byte_3 set_byte_3 [(7:0)] u8
}

bit2enum! {
	TimerValue
	Micro400 0
	Micro600 1
	Micro800 2
	Micro1600 3
}

bit2enum! {
	try BitClockDivider
	MHz125 0x3f
	Workaround 0x48
	MHz24 0xc
}

bit2enum! {
	try MessageSize
	N1 1
	N2 2
	N3 3
	N4 4
	N5 5
	N6 6
	N7 7
	N8 8
	N9 9
	N10 10
	N11 11
	N12 12
	N13 13
	N14 14
	N15 15
	N16 16
	N17 17
	N18 18
	N19 19
	N20 20
}

reg! {
	TransportControl
	enable set_enable [31] bool
	mode set_mode [(27:27)] TransportMode
	force_act set_force_act [25] bool
	enhanced_framing set_enhanced_framing [18] bool
	fdi_auto_train set_fdi_auto_train [15] bool
	link_training set_link_training [(try 10:8)] LinkTraining
	alternate_sr_scrambler set_alternate_sr_scrambler [6] bool
}

bit2enum! {
	TransportMode
	Sst 0
	Mst 1
}

bit2enum! {
	try LinkTraining
	Pattern1 0b000
	Pattern2 0b001
	Idle 0b010
	Normal 0b011
	Pattern3 0b100
}

reg! {
	DdiBufferControl
	enable set_enable [31] bool
	voltage_swing set_voltage_swing [(27:24)] u8 // FIXME u4
	port_reversal set_port_reversal [16] bool
	idle_status set_idle_status [7] bool
	a_lane_control set_a_lane_control [(4:4)] ALaneControl
	port_width set_port_width [(try 3:1)] PortWidth
	init_display_detected set_init_display_detected [0] bool
}

reg! {
	DdiBufferTranslation
	balance_leg_enable set_balance_leg_enable [31] bool
	// TODO de_emp_level, vref_sel, v_swing
}

bit2enum! {
	ALaneControl
	X2 0
	X4 1
}

bit2enum! {
	try PortWidth
	X1 0b000
	X2 0b001
	X4 0b011
}

reg! {
	PortClockSelect
	clock set_clock [(try 31:29)] PortClock
}

bit2enum! {
	try PortClock
	LcPll2700 0b000
	LcPll1350 0b001
	LcPll810  0b010
	SPll      0b011
	WrPll1    0b100
	WrPll2    0b101
	None      0b111
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Port {
	/// Known as DDI_AUX_{CTL,DATA} in vol2a and located at an entirely different address from the
	/// other registers. Don't ask me why.
	A,
	B,
	C,
	D,
	// FIXME This register does actually seem to exist though it isn't documented.
	E,
}

impl Port {
	fn offset_aux(&self) -> u32 {
		match self {
			Self::A => 0x64010,
			Self::B => 0xe4110,
			Self::C => 0xe4210,
			Self::D => 0xe4310,
			Self::E => 0xe4410,
		}
	}

	fn ctl_port(&self) -> u32 {
		self.offset_aux()
	}

	fn data_port(&self, instance: u8) -> u32 {
		assert!(instance < 5, "there are only 5 data registers per port");
		self.offset_aux() + 4 + u32::from(instance) * 4
	}

	unsafe fn load_ctl(&self, control: &mut Control) -> DpAuxCtl {
		DpAuxCtl(control.load(self.ctl_port()))
	}

	unsafe fn store_ctl(&self, control: &mut Control, ctl: DpAuxCtl) {
		control.store(self.ctl_port(), ctl.0)
	}

	unsafe fn load_data(&self, control: &mut Control, instance: u8) -> DpAuxData {
		DpAuxData(control.load(self.data_port(instance)))
	}

	unsafe fn store_data(&self, control: &mut Control, instance: u8, data: DpAuxData) {
		control.store(self.data_port(instance), data.0)
	}

	unsafe fn offset(&self) -> u32 {
		0x100
			* match self {
				Self::A => 0,
				Self::B => 1,
				Self::C => 2,
				Self::D => 3,
				Self::E => 4,
			}
	}

	impl_reg!(0x46100 PortClockSelect load_port_clk_sel store_port_clk_sel);
	impl_reg!(0x64040 TransportControl load_dp_tp_ctl store_dp_tp_ctl);
	impl_reg!(0x64000 DdiBufferControl load_ddi_buf_ctl store_ddi_buf_ctl);
}

/// Wait for an AUX CH reply.
unsafe fn aux_ch_wait_reply(control: &mut Control, port: Port) -> Result<DpAuxCtl, ReplyError> {
	loop {
		let mut ctl = port.load_ctl(control);
		if ctl.send_busy() {
			continue;
		}
		if ctl.time_out_error() {
			break Err(ReplyError::Timeout);
		}
		if ctl.receive_error() {
			break Err(ReplyError::Receive);
		}
		if ctl.done() {
			ctl.set_done(false);
			break Ok(ctl);
		}
	}
}

/// Read an AUX CH reply.
unsafe fn aux_ch_read_reply<'a>(
	control: &mut Control,
	port: Port,
	buf: &'a mut [u8; 20],
) -> Result<(DpAuxCtl, &'a [u8]), ReplyError> {
	let ctl = aux_ch_wait_reply(control, port)?;
	let buf = &mut buf[..ctl.message_size().unwrap() as usize];
	let mut it = buf.iter_mut();
	let mut data_i = 0;
	while let Some(b) = it.next() {
		let data = port.load_data(control, data_i);
		*b = data.byte_0();
		it.next().map(|b| *b = data.byte_1());
		it.next().map(|b| *b = data.byte_2());
		it.next().map(|b| *b = data.byte_3());
		data_i += 1;
	}
	Ok((ctl, buf))
}

/// Send initial I2C packet.
unsafe fn i2c_init(
	control: &mut Control,
	port: Port,
	address: u8,
	read: bool,
) -> Result<DpAuxCtl, I2CError> {
	let mut reply_buf = [0; 20];
	let mut i = 3u8;
	loop {
		let mut ctl = port.load_ctl(control);
		let mut data = DpAuxData(0);
		data.set_byte_0(i2c_format_request(read, true));
		data.set_byte_1(0);
		data.set_byte_2(address);
		ctl.set_message_size(MessageSize::N3);
		ctl.set_receive_error(true); // Clear error
		ctl.set_time_out_error(true); // Clear timeout
		ctl.set_send_busy(true);
		port.store_data(control, 0, data);
		port.store_ctl(control, ctl.clone());

		// Read reply
		match aux_ch_read_reply(control, port, &mut reply_buf).map_err(I2CError::ReplyError) {
			Ok((ctl, reply)) => {
				assert_eq!(reply.len(), 1, "TODO");
				match i2c_parse_reply(reply[0]).map_err(I2CError::I2CReplyError)? {
					I2CReply::I2CAcknowledged => return Ok(ctl),
					e => todo!("{:?}", e),
				}
			}
			Err(e) => {
				if let Some(ni) = i.checked_sub(1) {
					i = ni;
				} else {
					return Err(e);
				}
			}
		}
	}
}

/// Finish I2C transaction
unsafe fn i2c_finish(
	control: &mut Control,
	mut ctl: DpAuxCtl,
	port: Port,
	address: u8,
) -> Result<DpAuxCtl, I2CError> {
	// Send NACK to indicate end of I2C read
	let mut reply_buf = [0; 20];
	let mut i = 3u8;
	loop {
		let mut data = DpAuxData(0);
		data.set_byte_0(i2c_format_request(true, false));
		data.set_byte_1(0);
		data.set_byte_2(address);
		ctl.set_message_size(MessageSize::N3);
		ctl.set_send_busy(true);
		port.store_data(control, 0, data);
		port.store_ctl(control, ctl.clone());

		// Read reply
		match aux_ch_read_reply(control, port, &mut reply_buf).map_err(I2CError::ReplyError) {
			Ok((ctl, reply)) => {
				assert_eq!(reply.len(), 1, "TODO");
				match i2c_parse_reply(reply[0]).map_err(I2CError::I2CReplyError)? {
					I2CReply::I2CAcknowledged => return Ok(ctl),
					e => todo!("{:?}", e),
				}
			}
			Err(e) => {
				if let Some(ni) = i.checked_sub(1) {
					i = ni;
				} else {
					return Err(e);
				}
			}
		}
	}
}

/// Write a single byte of I2C data
unsafe fn i2c_put(
	control: &mut Control,
	mut ctl: DpAuxCtl,
	port: Port,
	address: u8,
	byte: u8,
) -> Result<DpAuxCtl, I2CError> {
	let mut reply_buf = [0; 20];
	let mut i = 3u8;
	loop {
		// Send request
		let mut data0 = DpAuxData(0);
		let mut data1 = DpAuxData(0);
		data0.set_byte_0(i2c_format_request(false, true));
		data0.set_byte_1(0);
		data0.set_byte_2(address);
		data0.set_byte_3(0); // length - 1
		data1.set_byte_0(byte);
		ctl.set_message_size(MessageSize::N5);
		ctl.set_send_busy(true);
		port.store_data(control, 0, data0);
		port.store_data(control, 1, data1);
		port.store_ctl(control, ctl.clone());

		// Wait for reply
		match aux_ch_read_reply(control, port, &mut reply_buf) {
			Ok((ctl, reply)) => {
				assert_eq!(reply, &[0]);
				return Ok(ctl);
			}
			Err(e) => {
				if let Some(ni) = i.checked_sub(1) {
					i = ni;
				} else {
					return Err(I2CError::ReplyError(e));
				}
			}
		}
	}
}

/// Read a single byte of I2C data
unsafe fn i2c_fetch(
	control: &mut Control,
	mut ctl: DpAuxCtl,
	port: Port,
	address: u8,
) -> Result<(DpAuxCtl, u8), I2CError> {
	let mut reply_buf = [0; 20];
	let mut i = 3u8;
	loop {
		// Send request
		let mut data = DpAuxData(0);
		data.set_byte_0(i2c_format_request(true, true));
		data.set_byte_1(0);
		data.set_byte_2(address);
		data.set_byte_3(0); // length - 1
		ctl.set_message_size(MessageSize::N4);
		ctl.set_send_busy(true);
		port.store_data(control, 0, data);
		port.store_ctl(control, ctl.clone());

		// Wait for reply
		match aux_ch_read_reply(control, port, &mut reply_buf) {
			Ok((ctl, reply)) => {
				assert_eq!(reply.len(), 2, "TODO");
				assert_eq!(reply[0], 0, "TODO");
				return Ok((ctl, reply[1]));
			}
			Err(e) => {
				if let Some(ni) = i.checked_sub(1) {
					i = ni;
				} else {
					return Err(I2CError::ReplyError(e));
				}
			}
		}
	}
}

/// Write I2C data byte-by-byte.
pub unsafe fn i2c_write(
	control: &mut Control,
	port: Port,
	address: u8,
	data: &[u8],
) -> Result<(), I2CError> {
	assert!(address <= 127, "invalid address");
	assert!(!data.is_empty(), "data may not be empty");

	let mut ctl = i2c_init(control, port, address, false)?;

	// Write data
	for &b in data {}
	todo!();

	Ok(())
}

/// Read I2C data byte-by-byte.
pub unsafe fn i2c_read(
	control: &mut Control,
	port: Port,
	address: u8,
	buf: &mut [u8],
) -> Result<(), I2CError> {
	assert!(address <= 127, "invalid address");
	assert!(!buf.is_empty(), "buf may not be empty");

	let mut ctl = i2c_init(control, port, address, true)?;
	for b in buf.iter_mut() {
		(ctl, *b) = i2c_fetch(control, ctl, port, address)?;
	}
	i2c_finish(control, ctl, port, address).map(|_| ())
}

/// Perform an I2C write immediately followed by a read.
pub unsafe fn i2c_write_read(
	control: &mut Control,
	port: Port,
	address: u8,
	data: &[u8],
	buf: &mut [u8],
) -> Result<(), I2CError> {
	assert!(address <= 127, "invalid address");
	assert!(!data.is_empty(), "data may not be empty");
	assert!(!buf.is_empty(), "buf may not be empty");

	let mut ctl = i2c_init(control, port, address, false)?;
	for b in data {
		ctl = i2c_put(control, ctl, port, address, *b)?;
	}

	// restart (REpeated START)
	ctl = i2c_init(control, port, address, true)?;
	for b in buf.iter_mut() {
		(ctl, *b) = i2c_fetch(control, ctl, port, address)?;
	}
	i2c_finish(control, ctl, port, address).map(|_| ())
}

#[derive(Debug)]
pub enum ReplyError {
	Receive,
	Timeout,
}

#[derive(Debug)]
pub enum I2CError {
	ReplyError(ReplyError),
	I2CReplyError(I2CReplyError),
}

fn i2c_format_request(read: bool, middle_of_transaction: bool) -> u8 {
	// bit 1:0 -> 00 = write, 01 = read, 10 = write status_request, 11 = reserved
	// bit 2   -> middle-of-transaction (MOT)
	// bit 3   -> 0 for I2C
	(u8::from(read) | u8::from(middle_of_transaction) << 2) << 4
}

fn i2c_parse_reply(data: u8) -> Result<I2CReply, I2CReplyError> {
	// bit 1:0 -> 00 = aux_ack, 01 = aux_nack, 10 = aux_defer, 11 = reserved
	// bit 3:2 -> 1:0 == aux_ack -> 00 = i2c_ack, 01 = i2c_nack, 10 = i2c_defer, 11 = reserved
	//            1:0 != aux_ack -> ignore (must be 00)
	if data & 0xf != 0 {
		log!("upper bits are not 0 ({:04b})", data & 0xf);
	}
	let data = data >> 4;
	match data & 3 {
		0 => match (data >> 2) & 3 {
			0 => Ok(I2CReply::I2CAcknowledged),
			1 => Ok(I2CReply::I2CNotAcknowledged),
			2 => Ok(I2CReply::I2CDefer),
			3 => Err(I2CReplyError::I2CReserved),
			_ => unreachable!(),
		},
		1 => Ok(I2CReply::AuxNotAcknowledged),
		2 => Ok(I2CReply::AuxDefer),
		3 => Err(I2CReplyError::AuxReserved),
		_ => unreachable!(),
	}
}

#[derive(Debug)]
pub enum I2CReplyError {
	I2CReserved,
	AuxReserved,
}

#[derive(Debug)]
pub enum I2CReply {
	I2CAcknowledged,
	I2CNotAcknowledged,
	I2CDefer,
	AuxNotAcknowledged,
	AuxDefer,
}

pub unsafe fn configure(control: &mut Control, port: Port, clock: PortClock) {
	// See vol11 p. 112 "Sequences for DisplayPort"

	// a. Configure Port Clock Select to direct the CPU Display PLL to the port
	set_port_clock(control, port, clock);

	// b. Configure and enable DP_TP_CTL with link training pattern 1 selected
	let mut tp = port.load_dp_tp_ctl(control);
	tp.set_enable(true);
	tp.set_mode(TransportMode::Sst);
	tp.set_fdi_auto_train(false);
	tp.set_link_training(LinkTraining::Pattern1);
	port.store_dp_tp_ctl(control, tp);

	// c. Configure DDI_BUF_TRANS. This can be done earlier if desired.
	// TODO do we really need to set up anything in DDI_BUF_TRANS?

	// d. Configure and enable DDI_BUF_CTL
	let mut ddi = port.load_ddi_buf_ctl(control);
	ddi.set_enable(true);
	port.store_ddi_buf_ctl(control, ddi);

	// e. Wait >518 us for buffers to enable before starting training or allow for longer time
	//    in TP1 before software timeout
	// FIXME how to check for DDI buffer status?
	rt::thread::sleep(core::time::Duration::from_millis(1));

	// f. Follow DisplayPort specification training sequence (see notes for failure handling)
	//
	// "For a closed, embedded connection, the DisplayPort transmitter and receiver may be set to pre-calibrated parameters without going through the full link training sequence. In this mode, the DisplayPort Source Device may start a normal operation without the AUX CH handshake for link training, as described in Section 2.5.3.3."
	if port != Port::A {
		todo!()
	}

	// g. If DisplayPort multi-stream - Set DP_TP_CTL link training to Idle Pattern, wait
	//    for 5 idle patterns (DP_TP_STATUS Min_Idles_Sent) (timeout after 800 us)
	// ergo skip

	// h. Set DP_TP_CTL link training to Normal, skip if eDP (DDI A)
	if port != Port::A {
		todo!()
	}
}

pub unsafe fn disable(control: &mut Control, port: Port) {
	// a. Disable DDI_BUF_CTL
	let mut bufctl = port.load_ddi_buf_ctl(control);
	bufctl.set_enable(false);
	port.store_ddi_buf_ctl(control, bufctl);

	// b. Disable DP_TP_CTL (do not set port to idle when disabling)
	let mut tpctl = port.load_dp_tp_ctl(control);
	tpctl.set_enable(false);
	port.store_dp_tp_ctl(control, tpctl);

	// c. Wait 8 us or poll on DDI_BUF_CTL Idle Status for buffers to return to idle
	while !port.load_ddi_buf_ctl(control).idle_status() {
		rt::thread::yield_now();
	}

	// TODO perform d. from here somehow

	// TODO disable port clock
}

pub unsafe fn set_port_clock(control: &mut Control, port: Port, clock: PortClock) {
	// Disable: e. Configure Port Clock Select to direct no clock to the port
	let mut v = port.load_port_clk_sel(control);
	v.set_clock(clock);
	port.store_port_clk_sel(control, v);
}

pub unsafe fn set_training_pattern(control: &mut Control, port: Port, pattern: LinkTraining) {
	let mut tp = port.load_dp_tp_ctl(control);
	tp.set_link_training(pattern);
	port.store_dp_tp_ctl(control, tp);
}
