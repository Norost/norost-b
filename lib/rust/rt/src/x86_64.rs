use core::arch::asm;

pub unsafe fn set_tls(tls: *const ()) {
	asm!("wrfsbase {tls}", tls = in(reg) tls);
}

pub unsafe fn read_tls_offset(offset: usize) -> usize {
	let data;
	asm!("mov {data}, fs:[{offset} * 8]", offset = in(reg) offset, data = out(reg) data);
	data
}

pub unsafe fn write_tls_offset(offset: usize, data: usize) {
	asm!("mov fs:[{offset} * 8], {data}", offset = in(reg) offset, data = in(reg) data);
}
