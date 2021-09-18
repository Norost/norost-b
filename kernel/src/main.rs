#![no_std]
#![no_main]
#![feature(asm)]

use core::panic::PanicInfo;

mod arch;
mod memory;

#[export_name = "main"]
pub extern "C" fn main() -> ! {
	let vga_buffer = 0xb8000 as *mut u8;

	for (i, &byte) in b"Oh wooooooow nice dude".iter().enumerate() {
		unsafe {
			*vga_buffer.offset(i as isize * 2) = byte;
			*vga_buffer.offset(i as isize * 2 + 1) = 0x9;
		}
	}

	unsafe {
		arch::init();
	}

	for (i, &byte) in b"QUACK QUACK QUACK".iter().enumerate() {
		unsafe {
			*vga_buffer.offset(i as isize * 2) = byte;
			*vga_buffer.offset(i as isize * 2 + 1) = 0xa;
		}
	}

	loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
