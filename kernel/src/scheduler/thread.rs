#[repr(C)]
pub struct Thread {
	gp_registers: [],
	fp_registers: Option<Box<[]>>,
	process: PID,
}
