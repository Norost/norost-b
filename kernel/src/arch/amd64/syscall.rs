use super::msr;
use crate::scheduler::process::Process;
use crate::scheduler::Thread;
use crate::scheduler::syscall;
use core::ptr::{self, NonNull};
use alloc::{boxed::Box, sync::{Arc, Weak}};

pub unsafe fn init() {
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

	// Set GS_BASE to a per-cpu structure
	let data = Box::leak(Box::<CpuData>::new_uninit());
	msr::wrmsr(msr::GS_BASE, data as *mut _ as u64);
}

pub unsafe fn set_current_thread(thread: Arc<Thread>) {
	// Remove reference to current thread.
	let old_thr: *const Thread;
	asm!("mov {0}, gs:[3 * 8]", lateout(reg) old_thr);
	if !old_thr.is_null() {
		Arc::from_raw(old_thr);
	}
	// Set reference to new thread.
	let user_stack = thread.user_stack.get().map_or_else(ptr::null_mut, NonNull::as_ptr);
	asm!("mov gs:[0 * 8], {0}", in(reg) user_stack);
	asm!("mov gs:[1 * 8], {0}", in(reg) thread.kernel_stack.get().as_ptr());
	asm!("mov gs:[2 * 8], {0}", in(reg) thread.process.as_ptr());
	asm!("mov gs:[3 * 8], {0}", in(reg) Arc::into_raw(thread));
}

/// Copy thread state from the CPU data to the thread.
pub unsafe fn save_current_thread_state() {
	let (us, ks, tr): (*mut _, *mut _, *const Thread);
	asm!("mov {0}, gs:[3 * 8]", lateout(reg) tr);
	let tr = &*tr;
	asm!("mov {0}, gs:[0 * 8]", lateout(reg) us);
	tr.user_stack.set(NonNull::new(us));
	asm!("mov {0}, gs:[1 * 8]", lateout(reg) ks);
	tr.kernel_stack.set(NonNull::new(ks).unwrap_unchecked());
}

#[repr(C)]
struct CpuData {
	user_stack_ptr: *mut usize,
	kernel_stack_ptr: *mut usize,
	process: *mut Process,
	thread: *const Thread,
}

#[naked]
unsafe extern "C" fn handler() {
	asm!("
		# Load kernel stack
		swapgs
		mov		gs:[0], rsp		# Save user stack ptr
		mov		rsp, gs:[8]		# Load kernel stack ptr

		# Save thread registers (except rax & rdx, we overwrite those anyways)
		push	r15
		push	r14
		push	r13
		push	r12
		push	r11
		push	r10
		push	r9
		push	r8
		push	rbp
		push	rdi
		push	rsi
		push	rdi
		push	rcx
		push	rbx

		# Check if the syscall ID is valid
		# Jump forward to take advantage of static prediction
		cmp		rax, {syscall_count}
		jae		1f

		# Call the appropriate handler
		# TODO figure out how to do this in one instruction
		lea		rcx, [rip + syscall_table]
		lea		rax, [rcx + rax * 8]
		mov		rcx, r10 # r10 is used as 4th parameter
		call	[rax]

	2:
		pop		rbx
		pop		rcx
		pop		rdi
		pop		rsi
		pop		rdi
		pop		rbp
		pop		r8
		pop		r9
		pop		r10
		pop		r11
		pop		r12
		pop		r13
		pop		r14
		pop		r15

		# Save kernel stack in case it got overwritten
		mov		gs:[8], rsp

		# Restore user stack pointer
		mov		rsp, gs:[0]

		# Swap to user GS for TLS
		swapgs

		# Go back to user mode
		rex64 sysret

		# Set error code and return
	1:
		mov		rax, -1
		xor		edx, edx
		jmp		2b
	", syscall_count = const syscall::SYSCALLS_LEN, options(noreturn));
}

pub fn current_process<'a>() -> &'a mut Process {
	unsafe {
		let process: *mut Process;
		asm!("mov {0}, gs:[0x10]", out(reg) process);
		&mut *process
	}
}

pub fn current_thread() -> Arc<Thread> {
	unsafe {
		let thread: *const Thread;
		asm!("mov {0}, gs:[0x18]", out(reg) thread);
		let r = Arc::from_raw(thread);
		let s = r.clone();
		let _ = Arc::into_raw(r);
		s
	}
}

pub fn current_thread_weak() -> Weak<Thread> {
	unsafe {
		let thread: *const Thread;
		asm!("mov {0}, gs:[0x18]", out(reg) thread);
		let r = Arc::from_raw(thread);
		let w = Arc::downgrade(&r);
		let _ = Arc::into_raw(r);
		w
	}
}
