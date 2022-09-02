use {
	super::{float::FloatStorage, msr},
	crate::{
		memory::{frame, Page},
		scheduler::{process::Process, syscall, Thread},
	},
	alloc::{
		boxed::Box,
		sync::{Arc, Weak},
	},
	core::{
		arch::asm,
		cell::{Cell, UnsafeCell},
		ptr::{self, NonNull},
	},
};

pub unsafe fn init(tss: &'static super::tss::TSS) {
	unsafe {
		// Enable syscall/sysenter
		msr::set_bits(msr::IA32_EFER, msr::IA32_EFER_SCE, true);

		// Set STAR kernel CS and user CS
		// Notes from OSDev wiki:
		// * SYSCALL loads CS from STAR 47:32
		// * It then loads SS from STAR 47:32 + 8.
		// * SYSRET loads CS from STAR 63:48. It loads EIP from ECX and SS from STAR 63:48 + 8.
		// * As well, in Long Mode, userland CS will be loaded from STAR 63:48 + 16 and userland
		//   SS from STAR 63:48 + 8 on SYSRET.
		msr::wrmsr(msr::STAR, (8 * 1) << 32 | (8 * 2 | 3) << 48);
		// Set LSTAR to handler
		msr::wrmsr(msr::LSTAR, handler as u64);
		// Ensure the interrupt flag is cleared on syscall enter
		msr::wrmsr(msr::SFMASK, 0x200);

		let mut cpu_stack = None;
		frame::allocate(1, |f| cpu_stack = Some(f), 0 as _, 0).unwrap();
		let cpu_stack = cpu_stack.unwrap().as_ptr();
		let cpu_stack_ptr = cpu_stack.cast::<Page>().wrapping_add(1).cast();

		// Set GS_BASE to a per-cpu structure
		let data = Box::leak(Box::new(CpuData {
			user_stack_ptr: ptr::null_mut(),
			kernel_stack_ptr: ptr::null_mut(),
			process: ptr::null_mut(),
			thread: ptr::null(),
			cpu_stack_ptr,
			tss,
		}));
		msr::wrmsr(msr::GS_BASE, data as *mut _ as u64);
	}
}

// Use repr(C) because manual offsets because there is literally no way to implement offset_of! in
// a sound way, i.e. it needs compiler support.
// https://github.com/rust-lang/rust/issues/48956
#[repr(C)]
pub struct CpuData {
	user_stack_ptr: *mut usize,
	kernel_stack_ptr: *mut usize,
	process: *const Process,
	thread: *const Thread,
	cpu_stack_ptr: *mut (),
	tss: &'static super::tss::TSS,
}

impl CpuData {
	pub const USER_STACK_PTR: usize = 0 * 8;
	pub const KERNEL_STACK_PTR: usize = 1 * 8;
	#[allow(dead_code)]
	pub const PROCESS: usize = 2 * 8;
	#[allow(dead_code)]
	pub const THREAD: usize = 3 * 8;
	#[allow(dead_code)]
	pub const CPU_STACK_PTR: usize = 4 * 8;
	#[allow(dead_code)]
	pub const TSS: usize = 5 * 8;
}

macro_rules! gs_load {
	(user_stack_ptr) => {{
		let v: *mut usize;
		core::arch::asm!("mov {}, gs:[0 * 8]", out(reg) v);
		v
	}};
	(kernel_stack_ptr) => {{
		let v: *mut usize;
		core::arch::asm!("mov {}, gs:[1 * 8]", out(reg) v);
		v
	}};
	(process) => {{
		let v: *const Process;
		core::arch::asm!("mov {}, gs:[2 * 8]", out(reg) v);
		v
	}};
	(thread) => {{
		let v: *const Thread;
		#[allow(unused_unsafe)]
		unsafe {
			core::arch::asm!("mov {}, gs:[3 * 8]", out(reg) v);
		}
		v
	}};
	(cpu_stack_ptr) => {{
		let v: *mut ();
		core::arch::asm!("mov {}, gs:[4 * 8]", out(reg) v);
		v
	}};
	(tss) => {
		#[allow(unused_unsafe)]
		unsafe {
			let v: *const super::tss::TSS;
			core::arch::asm!("mov {}, gs:[5 * 8]", out(reg) v);
			let v: &'static _ = &*v;
			v
		}
	};
}

macro_rules! gs_store {
	(user_stack_ptr = $val:expr) => {{
		let v: *mut usize = $val;
		core::arch::asm!("mov gs:[0 * 8], {}", in(reg) v);
	}};
	(kernel_stack_ptr = $val:expr) => {{
		let v: *mut usize = $val;
		core::arch::asm!("mov gs:[1 * 8], {}", in(reg) v);
	}};
	(process = $val:expr) => {{
		let v: *const Process = $val;
		core::arch::asm!("mov gs:[2 * 8], {}", in(reg) v);
	}};
	(thread = $val:expr) => {{
		let v: *const Thread = $val;
		core::arch::asm!("mov gs:[3 * 8], {}", in(reg) v);
	}};
	(cpu_stack_ptr = $val:expr) => {{
		let v: *mut () = $val;
		core::arch::asm!("mov gs:[4 * 8], {}", in(reg) v);
	}};
}

/// Copy thread state to the CPU local data.
pub unsafe fn set_current_thread(thread: Arc<Thread>) {
	unsafe {
		unref_current_thread();

		// Load fs, gs
		msr::wrmsr(msr::FS_BASE, thread.arch_specific.fs.get());
		msr::wrmsr(msr::KERNEL_GS_BASE, thread.arch_specific.gs.get());

		// Load float / vector registers
		(&mut *thread.arch_specific.float.get())
			.as_ref()
			.map(|o| o.restore());

		// Set reference to new thread.
		let user_stack = thread
			.user_stack
			.get()
			.map_or_else(ptr::null_mut, NonNull::as_ptr);
		gs_store!(user_stack_ptr = user_stack);
		gs_store!(kernel_stack_ptr = thread.kernel_stack.get().as_ptr());
		gs_load!(tss).set_rsp(0, thread.kernel_stack_top().as_ptr().cast());
		gs_store!(process = thread.process().map_or_else(ptr::null, Arc::as_ptr));
		gs_store!(thread = Arc::into_raw(thread));
	}
}

/// Copy thread state from the CPU data to the thread.
pub(super) unsafe fn save_current_thread_state() {
	debug_assert!(
		!super::interrupts_enabled(),
		"interrupts may not be enabled while switching threads"
	);
	unsafe {
		let thread = gs_load!(thread);
		if !thread.is_null() {
			let thread = &*thread;
			thread
				.user_stack
				.set(NonNull::new(gs_load!(user_stack_ptr)));
			thread
				.kernel_stack
				.set(NonNull::new(gs_load!(kernel_stack_ptr)).unwrap());

			// Save fs, gs
			thread.arch_specific.fs.set(msr::rdmsr(msr::FS_BASE));
			thread.arch_specific.gs.set(msr::rdmsr(msr::KERNEL_GS_BASE));

			// Save float / vector registers
			(&mut *thread.arch_specific.float.get())
				.as_mut()
				.map(|o| o.save());
		}
	}
}

pub struct ThreadData {
	fs: Cell<u64>,
	gs: Cell<u64>,
	// Kernel threads don't use the FPU so try to save a little memory.
	pub(super) float: UnsafeCell<Option<Box<FloatStorage>>>,
}

impl ThreadData {
	/// Initialize thread data for user thread.
	pub fn new_user() -> Self {
		Self {
			fs: 0.into(),
			gs: 0.into(),
			// Use Box::default to avoid having the compiler stupidly store FloatStorage on the
			// stack first.
			float: Some(Box::default()).into(),
		}
	}

	/// Initialize thread data for kernel thread.
	pub fn new_kernel() -> Self {
		Self { fs: 0.into(), gs: 0.into(), float: None.into() }
	}
}

#[naked]
unsafe extern "C" fn handler() {
	unsafe {
		asm!(
			// Load kernel stack
			"swapgs",
			"mov gs:[{user_stack_ptr}], rsp",
			"mov rsp, gs:[{kernel_stack_ptr}]",
			"sti",

			// Save thread registers (except rax & rdx, we overwrite those anyways)
			"push r11",
			"push r10",
			"push r9",
			"push r8",
			"push rdi",
			"push rsi",
			"push rcx",

			// Ensure stack is aligned to 16 bytes before call
			"sub rsp, 8",

			// Check if the syscall ID is valid
			"cmp rax, {syscall_count}",
			"jae 1f",
			// Call the appropriate handler
			"lea rcx, [rip + syscall_table]",
			"mov rax, [rcx + rax * 8]",
			"mov rcx, r10", // r10 is used as 4th parameter
			"call rax",
			"2:",

			"add rsp, 8",

			"pop rcx",
			"pop rsi",
			"pop rdi",
			"pop r8",
			"pop r9",
			"pop r10",
			"pop r11",

			// Restore user stack pointer & return
			"cli",
			"mov gs:[{kernel_stack_ptr}], rsp",
			"mov rsp, gs:[{user_stack_ptr}]",
			"swapgs",
			"sysretq",

			// Set error code and return
			"1:",
			"mov rax, -1",
			"xor edx, edx",
			"jmp 2b",
			syscall_count = const syscall::SYSCALLS_LEN,
			user_stack_ptr = const CpuData::USER_STACK_PTR,
			kernel_stack_ptr = const CpuData::KERNEL_STACK_PTR,
			options(noreturn)
		);
	}
}

pub fn current_process() -> Option<Arc<Process>> {
	unsafe {
		let process = gs_load!(process);
		(!process.is_null()).then(|| {
			let process = Arc::from_raw(process);
			// Intentionally leak as CpuData doesn't actually have ownership of the Arc.
			let _ = Arc::into_raw(process.clone());
			process
		})
	}
}

pub fn current_thread() -> Option<Arc<Thread>> {
	unsafe {
		current_thread_ptr().map(|thread| {
			let thread = Arc::from_raw(thread.as_ptr());
			// Intentionally leak as CpuData doesn't actually have ownership of the Arc.
			let _ = Arc::into_raw(thread.clone());
			thread
		})
	}
}

pub fn current_thread_weak() -> Option<Weak<Thread>> {
	unsafe {
		current_thread_ptr().map(|thread| {
			let thread = Arc::from_raw(thread.as_ptr());
			let weak = Arc::downgrade(&thread);
			let _ = Arc::into_raw(thread);
			weak
		})
	}
}

#[inline(always)]
pub fn current_thread_ptr() -> Option<NonNull<Thread>> {
	NonNull::new(gs_load!(thread) as *mut _)
}

pub(super) fn cpu_stack() -> *mut () {
	unsafe { gs_load!(cpu_stack_ptr) }
}

/// Clear the current thread & process from the local CPU data.
pub fn clear_current_thread() {
	unsafe {
		unref_current_thread();
		gs_store!(user_stack_ptr = ptr::null_mut());
		gs_store!(kernel_stack_ptr = ptr::null_mut());
		gs_store!(process = ptr::null_mut());
		gs_store!(thread = ptr::null_mut());
		gs_load!(tss).set_rsp(0, 0 as _);
	}
}

/// Remove reference to current thread.
unsafe fn unref_current_thread() {
	unsafe {
		let thread = gs_load!(thread);
		if !thread.is_null() {
			Arc::from_raw(thread);
		}
	}
}
