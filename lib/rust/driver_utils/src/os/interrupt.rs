use norostb_rt as rt;

pub fn allocate(irq: Option<u16>, mode: TriggerMode) -> rt::Object {
	let mut buf = [0; 32];
	buf[..10].copy_from_slice(b"interrupt/");
	let (mode, l) = match mode {
		TriggerMode::Edge => (&b"edge/"[..], 10 + 5),
		TriggerMode::Level => (&b"level/"[..], 10 + 6),
	};
	buf[10..l].copy_from_slice(mode);
	let l = l + match irq {
		Some(irq) => crate::util::u16_to_str(irq, 10, &mut buf[l..]),
		None => {
			buf[l..][..3].copy_from_slice(b"any");
			3
		}
	};
	rt::io::file_root().unwrap().create(&buf[..l]).unwrap()
}

#[derive(Clone, Copy, Debug)]
pub enum TriggerMode {
	Edge,
	Level,
}
