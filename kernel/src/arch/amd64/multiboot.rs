#[repr(C)]
struct Header {
	magic: u32,
	flags: u32,
	header_length: u32,
	checksum: u32,
}

#[link_section = ".multiboot"]
#[used]
static HEADER: Header = {
	let magic = 0xE85250D6;
	let flags = 0;
	let header_length = 0;
	let checksum = 0u32
		.wrapping_sub(magic)
		.wrapping_sub(flags)
		.wrapping_sub(header_length);
	Header { magic, flags, header_length, checksum }
};
