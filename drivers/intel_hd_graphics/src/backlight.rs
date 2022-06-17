use crate::control::Control;

reg! {
	PwmControl @ 0x48250
	enable set_enable [31] bool
}

reg! {
	SouthPwmControl1 @ 0x8250
	enable set_enable [31] bool
}

reg! {
	SouthPwmControl2 @ 0x8254
}

reg! {
	PanelPowerControl @ 0xc7204
	vdd_override set_vdd_override [3] bool
	backlight set_backlight [2] bool
	power_down_on_reset set_power_down_on_reset [1] bool
	power_state_target set_power_state_target [0] bool
}

reg! {
	PanelPowerStatus @ 0xc7200
	status set_status [31] bool
	power_sequence_progress set_power_sequence_progress [(try 29:28)] PowerSequenceProgress
	power_cycle_delay set_power_cycle_delay [27] bool
}

bit2enum! {
	try PowerSequenceProgress
	None 0b00
	PowerUp 0b01
	PowerDown 0b10
}

pub unsafe fn enable_panel(control: &mut Control) {
	// a. Enable panel power sequencing
	let mut v = PanelPowerControl(control.load(PanelPowerControl::REG));
	v.set_backlight(true);
	v.set_power_state_target(true);
	control.store(PanelPowerControl::REG, v.0);

	// b. Wait for panel power sequencing to reach the enabled state
	while !PanelPowerStatus(control.load(PanelPowerStatus::REG)).status() {
		rt::thread::yield_now();
	}
}

pub unsafe fn enable_backlight(control: &mut Control) {
	// l. If panel power sequencing is required - Enable panel backlight
	let mut v = PwmControl(control.load(PwmControl::REG));
	v.set_enable(true);
	control.store(PwmControl::REG, v.0);
}

pub unsafe fn disable(control: &mut Control) {
	let mut v = PwmControl(control.load(PwmControl::REG));
	v.set_enable(false);
	control.store(PwmControl::REG, v.0);

	// d. If panel power sequencing is required - Disable panel power
	let mut v = PanelPowerControl(control.load(PanelPowerControl::REG));
	v.set_backlight(false);
	v.set_power_state_target(false);
	control.store(PanelPowerControl::REG, v.0);

	// Wait for power off to complete
	while {
		let v = PanelPowerStatus(control.load(PanelPowerStatus::REG));
		v.status() || v.power_cycle_delay()
	} {
		rt::thread::yield_now();
	}
}
