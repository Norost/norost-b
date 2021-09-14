pub struct Process {
	address_space: AddressSpace,
	in_ports: Vec<Option<InPort>>,
	out_ports: Vec<Option<OutPort>>,
	named_ports: Box<[ReverseNamedPort]>,
	threads: Vec<NonNull<Thread>>,
}

pub struct ProcessID {
	index: u32,
}
