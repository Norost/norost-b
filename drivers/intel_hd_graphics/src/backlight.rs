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
	backlight set_backlight [2] bool
}

reg! {
	PanelPowerStatus @ 0xc7200
}

pub unsafe fn enable(control: &mut Control) {
	let mut v = PwmControl(control.load(PwmControl::REG));
	v.set_enable(true);
	control.store(PwmControl::REG, v.0);

	let mut v = PanelPowerControl(control.load(PanelPowerControl::REG));
	v.set_backlight(true);
	control.store(PanelPowerControl::REG, v.0);
}

pub unsafe fn disable(control: &mut Control) {
	let mut v = PwmControl(control.load(PwmControl::REG));
	v.set_enable(false);
	control.store(PwmControl::REG, v.0);

	let mut v = PanelPowerControl(control.load(PanelPowerControl::REG));
	v.set_backlight(false);
	control.store(PanelPowerControl::REG, v.0);
}
