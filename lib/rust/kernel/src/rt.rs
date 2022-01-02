use crate::syscall;
use core::arch::asm;
use core::panic::PanicInfo;
use core::time::Duration;

#[naked]
#[export_name = "_start"]
unsafe extern "C" fn start() {
	asm!(
		"
		lea		rsp, [rip + {stack} + 16 * 4096]
		call	main
		mov		eax, 6
		xor		edi, edi
		syscall
		",
		stack = sym STACK,
		options(noreturn)
	);
}

#[derive(Clone, Copy)]
#[repr(align(4096))]
struct P([u8; 4096]);
static mut STACK: [P; 16] = [P([0; 4096]); 16];

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
	syslog!("Panic! {:#?}", info);
	loop {
		syscall::sleep(Duration::MAX);
	}
}
