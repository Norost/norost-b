use {
	alloc::string::ToString,
	core::{num::NonZeroUsize, ptr::NonNull, str},
	norostb_rt as rt,
};

pub fn alloc_dma(size: NonZeroUsize) -> rt::io::Result<(NonNull<u8>, u64, NonZeroUsize)> {
	let (buf, buf_phys) = alloc_dma_object(size)?;
	let (buf, buf_size) = buf.map_object(None, rt::io::RWX::RW, 0, usize::MAX)?;
	Ok((buf, buf_phys, buf_size.try_into().unwrap()))
}

pub fn alloc_dma_object(size: NonZeroUsize) -> rt::io::Result<(rt::Object, u64)> {
	let size = size.to_string();
	let root = rt::io::file_root().unwrap();
	let buf = root.open(b"dma")?.create(size.as_bytes())?;
	let mut r = [0; 32];
	let r_len = buf.open(b"phys").unwrap().read(&mut r).unwrap();
	let buf_phys = str::from_utf8(&r[..r_len]).unwrap().parse::<u64>().unwrap();
	Ok((buf, buf_phys))
}
