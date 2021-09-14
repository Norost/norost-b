pub struct AsidList {
	list: Box<[PID]>,
	in_use: u16,
}

static mut LIST: Option<AsidList> = None;
