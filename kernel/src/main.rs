#![no_std]
#![no_main]
#![feature(asm)]
#![feature(maybe_uninit_extra, maybe_uninit_uninit_array)]

use core::fmt::Write;
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

	// Simple test program meant to be loaded at 0x1000
	#[rustfmt::ignore]
	let program = [
		0x86, 0xc0, // xchg al, al
		0x86, 0xdb, // xchg bl, bl
		0x86, 0xc9, // xchg cl, cl
		0x86, 0xd2, // xchg dl, dl
		0xeb, 0xf6, // jmp  rip-10
	];
	let frame = memory::frame::allocate_contiguous(1).unwrap();
	for (i, b) in program.iter().copied().enumerate() {
		unsafe {
			*frame.as_ptr().cast::<u8>().add(i) = b;
		}
	}

	let mut process = scheduler::process::Process::new().unwrap();
	unsafe {
		process.add_frames(0x1000 as *const _, Some(frame).into_iter()).unwrap();
	}

	process.run();
	unsafe { let _ = core::ptr::read_volatile(0x0 as *const u8); }
	unsafe { asm!("ud2") };

	dbg!(memory::r#virtual::DumpCurrent);

	power::halt();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	let mut vga = driver::vga::text::Text::new();
	fatal!("Panic!");
	fatal!("  {:?}", info);
	power::halt();
}
