#![no_std]
#![no_main]
#![feature(asm)]
#![feature(maybe_uninit_extra, maybe_uninit_uninit_array)]

use core::fmt::Write;
use core::panic::PanicInfo;

mod arch;
mod boot;
mod driver;
mod memory;
mod sync;

#[export_name = "main"]
pub extern "C" fn main(boot_info: &boot::Info) -> ! {
	let mut vga = driver::vga::text::Text::new();
	writeln!(vga, "Hello motherfucker! {:#?}", boot_info);

	writeln!(vga, "Free memory: {:?}", memory::frame::free_memory());

	for region in boot_info.memory_regions() {
		use memory::{Page, frame::{MemoryRegion, PPN}};
		let (base, size) = (region.base as usize, region.size as usize);
		let align = (Page::SIZE - base % Page::SIZE) % Page::SIZE;
		let base = base + align;
		let count = (size - align) / Page::SIZE;
		if let Ok(base) = PPN::try_from_usize(base) {
			let region = MemoryRegion {
				base,
				count,
			};
			writeln!(vga, "Add {:?}", &region);
			unsafe {
				memory::frame::add_memory_region(region);
			}
		}
	}

	writeln!(vga, "Free memory: {:?}", memory::frame::free_memory());

	let cb = |p| { writeln!(vga, "Allocated page: {:?}", p); };
	memory::frame::allocate(3, cb, core::ptr::null(), 0);

	unsafe {
		arch::init();
	}

	vga.write_str(b"QUACK QUACK QUACK\n", 0xa, 0x0);

	loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	let mut vga = driver::vga::text::Text::new();
	vga.write_str(b"PANIC!\n", 0xc, 0x0);
	let _ = writeln!(vga, "{:?}", info);
	loop {}
}
