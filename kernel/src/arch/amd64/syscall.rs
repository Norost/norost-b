use super::msr;

pub unsafe fn init() {
	dbg!(handler as *const u8);

	// Enable syscall/sysenter
	msr::set_bits(msr::IA32_EFER, msr::IA32_EFER_SCE, true);

	// Set STAR kernel CS and user CS
	// Notes from OSDev wiki:
	// * SYSCALL loads CS from STAR 47:32
	// * It then loads SS from STAR 47:32 + 8.
	// * SYSRET loads CS from STAR 63:48. It loads EIP from ECX and SS from STAR 63:48 + 8. 
	// * As well, in Long Mode, userland CS will be loaded from STAR 63:48 + 16 on SYSRET and
	//   userland SS will be loaded from STAR 63:48 + 8
	msr::wrmsr(msr::STAR, (8 * 1) << 32 | (8 * 2) << 48);
	// Set LSTAR to handler
	//wrmsr(0xc0000082, handler as u32, (handler as u64 >> 32) as u32);
	msr::wrmsr(msr::LSTAR, handler as u64);

	dbg!(msr::rdmsr(0xc0000081) as *const u8);
	dbg!(msr::rdmsr(0xc0000082) as *const u8);
}

#[export_name = "mini_test_stack"]
#[used]
static mut MINI_STACK: [usize; 1024] = [0; 1024];

#[naked]
unsafe fn handler() {
	asm!("
	.l:
		lea		rsp, [rip + mini_test_stack + 0x1000]
		push	r11
		push	rcx
		call	syscall_test
		pop		rcx
		pop		r11
		rex64 sysret
	", options(noreturn));
}

#[export_name = "syscall_test"]
unsafe fn test() {
	dbg!("I work!");
}
