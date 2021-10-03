use super::msr;
use crate::scheduler::syscall;

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
		# Save thread registers
		lea		rsp, [rip + mini_test_stack + 0x1000]
		push	r11
		push	rcx

		# Check if the syscall ID is valid
		# Jump forward to take advantage of static prediction
		cmp		rax, {syscall_count}
		jae		.bad_syscall_id

		# Call the appriopriate handler
		# TODO figure out how to do this in one instruction
		lea		rcx, [rip + syscall_table]
		lea		rcx, [rcx + rax * 8]
		call	[rcx]

	.return:
		pop		rcx
		pop		r11
		rex64 sysret

		# Set error code and return
	.bad_syscall_id:
		mov		rax, -1
		xor		edx, edx
		jmp		.return
	", syscall_count = const syscall::SYSCALLS_LEN, options(noreturn));
}
