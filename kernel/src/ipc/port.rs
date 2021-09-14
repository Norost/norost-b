pub struct InPort {
	process: PID,
	index: u32,
}

pub struct OutPort {
	process: PID,
	index: u32,
}

pub struct NamedPort {
	name: Box<str>,
	process: PID,
}

pub struct ReverseNamedPort {
	index: u32,
}
