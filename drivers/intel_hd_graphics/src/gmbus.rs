//! # GMBUS
//!
//! GMBUS uses the I2C protocol for communication.

use crate::control::Control;

reg! {
	Gmbus0 @ 0xc5100
	rate set_rate [(try 10:8)] Rate
	pin_pair set_pin_pair [(try 2:0)] PinPair
}

bit2enum! {
	try Rate
	Hz100K 0b000
	Hz50K 0b001
}

bit2enum! {
	try PinPair
	None 0b000
	DacDdc 0b010
	DdiC 0b100
	DdiB 0b101
	DdiD 0b110
}

reg! {
	Gmbus1 @ 0xc5104
	software_clear_interrupt set_software_clear_interrupt [31] bool
	software_ready set_software_ready [30] bool
	enable_timeout set_enable_timeout [29] bool
	bus_cycle set_bus_cycle [(try 27:25)] BusCycle
	byte_count set_byte_count [(24:16)] u16 // FIXME the data is 9 bits
	slave_register_index set_slave_register_index [(15:8)] u8
	slave_address set_slave_address [(7:1)] u8
	slave_read set_slave_read [0] bool
}

bit2enum! {
	try BusCycle
	NoCycle 0b000
	Wait 0b001
	IndexWait 0b011
	GenStop 0b100
	Stop 0b101
	IndexStop 0b111
}

reg! {
	Gmbus2 @ 0xc5108
	inuse set_inuse [15] bool
	hardware_wait set_hardware_wait [14] bool
	slave_stall_timeout set_slave_stall_timeout [13] bool
	gmbus_interrupt_status set_gmbus_interrupt_status [12] bool
	hardware_ready set_hardware_ready [11] bool
	nak_indicator set_nak_indicator [10] bool
	gmbus_active set_gmbus_active [9] bool
	current_byte_count set_current_byte_count [(8:0)] u16 // FIXME the data is 9 bits
}

reg! {
	Gmbus3 @ 0xc510c
	byte_3 set_byte_3 [(31:24)] u8
	byte_2 set_byte_2 [(23:16)] u8
	byte_1 set_byte_1 [(15:8)] u8
	byte_0 set_byte_0 [(7:0)] u8
}

reg! {
	Gmbus4 @ 0xc5110
	//interrupt_mask set_interrupt_mask [(4:0)] TODO
}

reg! {
	Gmbus5 @ 0xc5120
	two_byte_index_enable set_two_byte_index_enable [31] bool
	two_byte_slave_index set_two_byte_slave_index [(15:0)] u16
}

pub unsafe fn read(control: &mut Control, slave: u16, buf: &mut [u8]) -> Result<(), GmbusError> {
	assert!(buf.len() <= 256, "buf too large");

	let (addr, extra) = slave_addr(slave);
	log!("rd 0");
	prepare_io(control, addr, buf.len().try_into().unwrap(), true)?;
	log!("rd 1");

	if let Some(b) = extra {
		let mut out = Gmbus3::from_raw(0);
		out.set_byte_0(b);
		control.store(Gmbus3::REG, out.as_raw());
		wait_progress(control)?;
		log!("rd 2");
	}

	let mut iter = buf.iter_mut();
	while let Some(b) = iter.next() {
		//log!("rd 3");
		wait_progress(control)?;
		let inr = Gmbus3::from_raw(control.load(Gmbus3::REG));
		let status = Gmbus2::from_raw(control.load(Gmbus2::REG));
		log!("rd 4 {} {:08x}", status.current_byte_count(), inr.as_raw());
		*b = inr.byte_0();
		iter.next().map(|b| *b = inr.byte_1());
		iter.next().map(|b| *b = inr.byte_2());
		iter.next().map(|b| *b = inr.byte_3());
		rt::thread::sleep(core::time::Duration::from_secs(2));
	}

	//log!("rd 5");
	//wait_complete(control)?;
	stop_transaction(control);
	//log!("rd 6");
	Ok(())
}

pub unsafe fn write(control: &mut Control, slave: u16, data: &[u8]) -> Result<(), GmbusError> {
	assert!(data.len() <= 256, "data too large");

	let (addr, extra) = slave_addr(slave);
	let mut iter = extra.iter().chain(data);
	let mut write_data = |control: &mut Control| {
		if let Some(&b) = iter.next() {
			let mut out = Gmbus3::from_raw(0);
			out.set_byte_0(b);
			iter.next().map(|&b| out.set_byte_1(b));
			iter.next().map(|&b| out.set_byte_2(b));
			iter.next().map(|&b| out.set_byte_3(b));
			control.store(Gmbus3::REG, out.as_raw());
			true
		} else {
			false
		}
	};

	log!("wr 0");
	// According to managarm we need to prepare data before setting the command
	write_data(control);
	prepare_io(control, addr, data.len().try_into().unwrap(), false)?;
	wait_progress(control)?;
	log!("wr 1");

	while write_data(control) {
		log!("wr 2");
		wait_progress(control)?;
		log!("wr 3");
	}

	log!("wr 4");
	//wait_complete(control)?;
	stop_transaction(control);
	log!("wr 5");
	Ok(())
}

unsafe fn prepare_io(
	control: &mut Control,
	addr: u8,
	len: u16,
	read: bool,
) -> Result<(), GmbusError> {
	let mut cmd = Gmbus1(0);
	log!("gmbus2 {:#06x}", control.load(Gmbus2::REG));
	cmd.set_slave_read(read);
	cmd.set_slave_address(addr);
	cmd.set_byte_count(len);
	cmd.set_bus_cycle(BusCycle::IndexWait);
	cmd.set_slave_register_index(0);
	cmd.set_software_ready(true);
	log!("gmbus1 {:06x} {}", cmd.as_raw(), cmd.byte_count());
	control.store(Gmbus1::REG, cmd.as_raw());
	let cmd = Gmbus1::from_raw(control.load(Gmbus1::REG));
	log!("gmbus0 {:08x}", control.load(Gmbus0::REG));
	log!("gmbus1 {:08x} {}", cmd.as_raw(), cmd.byte_count());
	log!("gmbus2 {:08x}", control.load(Gmbus2::REG));
	Ok(())
}

unsafe fn slave_addr(slave: u16) -> (u8, Option<u8>) {
	match slave {
		0..=127 => (slave as u8, None),
		128..=1023 => (0b11110_00 | (slave >> 8) as u8, Some(slave as u8)),
		_ => panic!("slave address is not 10 bit"),
	}
}

// ~3G / 1M = ~3K, which should be plenty of iterations for a 50K/100K Hz I2C bus.
const MAX_TRY_ITERATIONS: usize = 1 << 20;

unsafe fn wait_progress(control: &mut Control) -> Result<(), GmbusError> {
	for _ in 0..MAX_TRY_ITERATIONS {
		let s = Gmbus2::from_raw(control.load(Gmbus2::REG));
		//log!("| {:04x}", s.as_raw());
		if s.nak_indicator() {
			return Err(GmbusError::NotAcknowledged);
		}
		if s.hardware_ready() {
			return Ok(());
		}
	}
	log!("gmbus2 timeout {:08x}", control.load(Gmbus2::REG));
	Err(GmbusError::Timeout)
}

unsafe fn wait_complete(control: &mut Control) -> Result<(), GmbusError> {
	for _ in 0..MAX_TRY_ITERATIONS {
		let s = Gmbus2::from_raw(control.load(Gmbus2::REG));
		if s.nak_indicator() {
			return Err(GmbusError::NotAcknowledged);
		}
		if s.hardware_wait() {
			return Ok(());
		}
	}
	Err(GmbusError::Timeout)
}

unsafe fn stop_transaction(control: &mut Control) {
	let mut cmd = Gmbus1::from_raw(0);
	cmd.set_software_ready(true);
	cmd.set_bus_cycle(BusCycle::GenStop);
	control.store(Gmbus1::REG, cmd.as_raw());
}

#[derive(Debug)]
pub enum GmbusError {
	Timeout,
	NotAcknowledged,
}
