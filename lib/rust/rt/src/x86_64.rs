use core::arch::asm;

pub unsafe fn set_tls(tls: *mut ()) {
	unsafe {
		asm!("wrfsbase {tls}", tls = in(reg) tls);
	}
}

pub unsafe fn get_tls() -> *mut () {
	let tls: *mut ();
	unsafe {
		asm!("rdfsbase {tls}", tls = out(reg) tls);
	}
	tls
}

pub unsafe fn read_tls_offset(offset: usize) -> usize {
	let data;
	unsafe {
		asm!("mov {data}, fs:[{offset} * 8]", offset = in(reg) offset, data = out(reg) data);
	}
	data
}

pub unsafe fn write_tls_offset(offset: usize, data: usize) {
	unsafe {
		asm!("mov fs:[{offset} * 8], {data}", offset = in(reg) offset, data = in(reg) data);
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
	unsafe {
		// rax: thread handle
		// rsp: pointer to program arguments & environment variables
		asm!(
			// Allocate stack space manually so the OS provides a guard page for us.
			"mov eax, {alloc}",
			"xor edi, edi", // Any base
			"mov esi, 1 << 16", // 64 KiB ought to be enough for now.
			"mov edx, 4 | 2", // RW
			"syscall",

			// The program arguments are located at $rsp
			"mov rdi, rsp",
			// The stack is located at $rdx, if successful
			// $rax denotes tha actual amount of allocated memory
			"lea rsp, [rdx + rax]",
			// Only jump if stack allocation did *not* fail, i.e. $rax is not negative
			"test eax, eax",
			"jns {start}",

			// Exit (abort) immediately as a last resort
			"mov eax, {exit}",
			"mov edi, 130", // Exit code
			"syscall",
			start = sym super::rt_start,
			alloc = const norostb_kernel::syscall::ID_ALLOC,
			exit = const norostb_kernel::syscall::ID_EXIT,
			options(noreturn),
		);
	}
}

// See lib.rs
#[linkage = "weak"]
#[export_name = "memcpy"]
#[naked]
unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
	unsafe {
		asm!(
			"mov rax, rdi",
			"mov rcx, rdx",
			"rep movsb",
			"ret",
			options(noreturn),
		);
	}
}

// See lib.rs
#[linkage = "weak"]
#[export_name = "memmove"]
#[naked]
unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
	unsafe {
		asm!(
			"mov rax, rdi",
			"mov rcx, rdx",
			"cmp rsi, rdi",
			"jl	2f",
			// rsi > rdi -> copy lowest first
			"rep movsb",
			"ret",
			"2:",
			// rsi < rdi -> copy highest first
			"std",
			"lea rsi, [rsi + rcx - 1]",
			"lea rdi, [rdi + rcx - 1]",
			"rep movsb",
			"cld",
			"ret",
			options(noreturn),
		);
	}
}

// See lib.rs
#[linkage = "weak"]
#[export_name = "memset"]
#[naked]
unsafe extern "C" fn memset(dest: *mut u8, c: u8, n: usize) -> *mut u8 {
	unsafe {
		asm!(
			"mov rcx, rdx",
			"xchg rax, rsi",
			"rep stosb",
			"mov rax, rsi",
			"ret",
			options(noreturn),
		);
	}
}

// See lib.rs
#[linkage = "weak"]
#[export_name = "memcmp"]
#[naked]
unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
	unsafe {
		// rep cmpsb is very slow, so implement something manually
		//  - benchmark on 2G data: 65ms manual version vs 810ms rep cmpsb
		asm!(
			"mov r8, rdx",
			"and r8, ~0x7",
			"add r8, rdi",
			"add rdx, rdi",
			// make equal so if n == 0 then eax - ecx == 0 too
			"mov eax, ecx",
			// Compare in chunks of 8 bytes
			"jmp 3f",
			"2:",
			"mov rax, QWORD PTR [rdi]",
			// if non-zero, one of the bytes differs
			// don't increase rdi/rsi & rescan with byte loads
			"cmp rax, QWORD PTR [rsi]",
			"jne 4f",
			// see above
			"add rdi, 8",
			"add rsi, 8",
			"3:",
			"cmp rdi, r8",
			"jnz 2b",
			"4:",
			// Compare individual bytes
			"jmp 3f",
			"2:",
			"movsx eax, BYTE PTR [rdi]",
			"movsx ecx, BYTE PTR [rsi]",
			"cmp eax, ecx",
			"jne 4f",
			"inc rdi",
			"inc rsi",
			"3:",
			"cmp rdi, rdx",
			"jnz 2b",
			"4:",
			"sub eax, ecx",
			"ret",
			options(noreturn),
		);
	}
}
