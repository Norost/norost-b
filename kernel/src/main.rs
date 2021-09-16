#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod arch;

#[export_name = "main"]
pub extern "C" fn main() -> ! {
	let vga_buffer = 0xb8000 as *mut u8;

	for (i, &byte) in b"Hello, world!".iter().enumerate() {
		unsafe {
			*vga_buffer.offset(i as isize * 2) = byte;
			*vga_buffer.offset(i as isize * 2 + 1) = 0xb;
		}
	}

	loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
