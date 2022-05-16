use core::{arch::asm, mem};

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
unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
	unsafe {
		#[inline]
		unsafe fn cmp<T, U, F>(mut a: *const T, mut b: *const T, n: usize, f: F) -> i32
		where
			T: Clone + Copy + Eq,
			U: Clone + Copy + Eq,
			F: FnOnce(*const U, *const U, usize) -> i32,
		{
			for _ in 0..n / mem::size_of::<T>() {
				unsafe {
					if a.read_unaligned() != b.read_unaligned() {
						return f(a.cast(), b.cast(), mem::size_of::<T>());
					}
					a = a.add(1);
					b = b.add(1);
				}
			}
			f(a.cast(), b.cast(), n % mem::size_of::<T>())
		}
		let c1 = |mut a: *const u8, mut b: *const u8, n| {
			for _ in 0..n {
				if a.read() != b.read() {
					return i32::from(a.read()) - i32::from(b.read());
				}
				a = a.add(1);
				b = b.add(1);
			}
			0
		};
		let c2 = |a: *const u16, b, n| cmp(a, b, n, c1);
		let c4 = |a: *const u32, b, n| cmp(a, b, n, c2);
		let c8 = |a: *const u64, b, n| cmp(a, b, n, c4);
		let c16 = |a: *const u128, b, n| cmp(a, b, n, c8);
		let c32 = |a: *const [u128; 2], b, n| cmp(a, b, n, c16);
		c32(a.cast(), b.cast(), n)
	}
}
