#![no_std]
#![no_main]
#![feature(asm)]
#![feature(maybe_uninit_extra, maybe_uninit_uninit_array)]
#![feature(naked_functions)]

use core::panic::PanicInfo;

#[macro_use]
mod log;

mod arch;
mod boot;
mod driver;
mod ipc;
mod memory;
mod power;
mod scheduler;
mod sync;

#[export_name = "main"]
pub extern "C" fn main(boot_info: &boot::Info) -> ! {
	log::init();

	for region in boot_info.memory_regions() {
		use memory::{
			frame::{MemoryRegion, PPN},
			Page,
		};
		let (base, size) = (region.base as usize, region.size as usize);
		let align = (Page::SIZE - base % Page::SIZE) % Page::SIZE;
		let base = base + align;
		let count = (size - align) / Page::SIZE;
		if let Ok(base) = PPN::try_from_usize(base) {
			let region = MemoryRegion { base, count };
			unsafe {
				memory::frame::add_memory_region(region);
			}
		}
	}

	unsafe {
		memory::r#virtual::init();
		arch::init();
	}

	assert!(!boot_info.drivers().is_empty(), "no drivers");

	for driver in boot_info.drivers() {
		let mut process = scheduler::process::Process::from_elf(driver.as_slice()).unwrap();
		process.run();
	}
	unsafe {
		let _ = core::ptr::read_volatile(0x0 as *const u8);
	}

	power::halt();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	fatal!("Panic!");
	fatal!("  {:?}", info);
	power::halt();
}
