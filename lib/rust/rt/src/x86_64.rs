use core::arch::asm;

pub unsafe fn set_tls(tls: *mut ()) {
	unsafe {
		asm!(
			"wrfsbase {tls}",
			tls = in(reg) tls,
			options(nostack, preserves_flags),
		);
	}
}

pub unsafe fn get_tls() -> *mut () {
	let tls: *mut ();
	unsafe {
		asm!(
			"rdfsbase {tls}",
			tls = out(reg) tls,
			options(nostack, readonly, preserves_flags),
		);
	}
	tls
}

pub unsafe fn read_tls_offset(offset: usize) -> usize {
	let data;
	unsafe {
		asm!(
			"mov {data}, fs:[{offset} * 8]",
			offset = in(reg) offset,
			data = out(reg) data,
			options(nostack, readonly, preserves_flags),
		);
	}
	data
}

pub unsafe fn write_tls_offset(offset: usize, data: usize) {
	unsafe {
		asm!(
			"mov fs:[{offset} * 8], {data}",
			offset = in(reg) offset,
			data = in(reg) data,
			options(nostack, preserves_flags),
		);
	}
}

// SysV ABI:
// - Parameter: rdi, rsi, rdx, ...
// - Return: rax, rdx
// - Scratch: rcx, ...
// - DF is cleared by default

// See lib.rs
#[linkage = "weak"]
#[export_name = "_start"]
#[naked]
extern "C" fn _start() -> ! {
	const _: () = assert!(
		norostb_kernel::syscall::ID_ALLOC == 0,
		"xor optimization is invalid"
	);
	unsafe {
		// rax: thread handle
		// rsp: pointer to program arguments & environment variables
		asm!(
			// Allocate stack space manually so the OS provides a guard page for us.
			//"mov eax, {alloc}",
			"xor eax, eax", // ID_ALLOC
			"xor edi, edi", // Any base
			"mov esi, 1 << 16", // 64 KiB ought to be enough for now.
			"mov edx, 4 | 2", // RW
			"syscall",

			// The program arguments are located at $rsp
			"mov rdi, rsp",
			// The stack is located at $rdx, if successful
			// $rax denotes tha actual amount of allocated memory
			// Substract 8 since pages are at least 4096 bytes and the stack must be
			// 16-byte aligned *before* "calling" (in our case, we jump)
			"lea rsp, [rdx + rax - 8]",
			// Only jump if stack allocation did *not* fail, i.e. $rax is not negative
			"test rax, rax",
			"jns {start}",

			// Exit (abort) immediately as a last resort
			"mov eax, {exit}",
			"mov edi, 130", // Exit code
			"syscall",
			start = sym super::rt_start,
			//alloc = const norostb_kernel::syscall::ID_ALLOC,
			exit = const norostb_kernel::syscall::ID_EXIT,
			options(noreturn),
		);
	}
}
