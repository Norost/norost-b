use alloc::string::ToString;
use core::{num::NonZeroUsize, ptr::NonNull, str};
use norostb_rt as rt;

pub fn alloc_dma(size: NonZeroUsize) -> rt::io::Result<(NonNull<u8>, u64, NonZeroUsize)> {
	let size = size.to_string();
	let root = rt::io::file_root().unwrap();
	let buf = root.open(b"dma")?.create(size.as_bytes())?;
	let mut r = [0; 32];
	let r_len = buf.open(b"phys").unwrap().read(&mut r).unwrap();
	let buf_phys = str::from_utf8(&r[..r_len]).unwrap().parse::<u64>().unwrap();
	let r_len = buf.open(b"size").unwrap().read(&mut r).unwrap();
	let buf_size = str::from_utf8(&r[..r_len])
		.unwrap()
		.parse::<NonZeroUsize>()
		.unwrap();
	let buf = buf.map_object(None, 0, buf_size.get())?;
	Ok((buf, buf_phys, buf_size))
}
