use super::process::Process;
use crate::arch;
use crate::memory::frame;
use crate::time::Monotonic;
use core::cell::Cell;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;
use alloc::sync::{Arc, Weak};

pub struct Thread {
	pub user_stack: Cell<Option<NonNull<usize>>>,
	pub kernel_stack: Cell<NonNull<usize>>,
	pub process: NonNull<Process>,
	sleep_until: Cell<Monotonic>,
}

impl Thread {
	pub fn new(start: usize, process: NonNull<Process>) -> Result<Self, frame::AllocateContiguousError> {
		unsafe {
			let kernel_stack_base = frame::allocate_contiguous(NonZeroUsize::new(1).unwrap())?
				.as_ptr()
				.cast::<[usize; 512]>();
			let mut kernel_stack = kernel_stack_base.add(1).cast::<usize>();
			let mut push = |val: usize| {
				kernel_stack = kernel_stack.sub(1);
				kernel_stack.write(val);
			};
			push(4 * 8 | 3); // ss
			push(0); // rsp
		 //push(0x202);     // rflags: Set reserved bit 1, enable interrupts (IF)
			push(0x2); // rflags: Set reserved bit 1
			push(3 * 8 | 3); // cs
			push(start); // rip
			 // Reserve space for (zeroed) registers
			kernel_stack = kernel_stack.sub(15);
			Ok(Self {
				user_stack: Cell::new(None),
				kernel_stack: Cell::new(NonNull::new(kernel_stack).unwrap()),
				process,
				sleep_until: Cell::new(Monotonic::ZERO),
			})
		}
	}

	/// Suspend the currently running thread & begin running this thread.
	pub fn resume(self: Arc<Self>) -> ! {
		unsafe { self.process.as_ref().activate_address_space() };

		unsafe {
			crate::arch::amd64::set_current_thread(self);
		}

		// iretq is the only way to preserve all registers
		unsafe {
			asm!("
				# Set kernel stack
				mov		rsp, gs:[8]

				# TODO is this actually necessary?
				mov		ax, (4 * 8) | 3		# ring 3 data with bottom 2 bits set for ring 3
				mov		ds, ax
				mov 	es, ax

				# Don't execute swapgs if we're returning to kernel mode (1)
				mov		eax, [rsp + 15 * 8 + 1 * 8]	# CS
				and		eax, 3

				pop		r15
				pop		r14
				pop		r13
				pop		r12
				pop		r11
				pop		r10
				pop		r9
				pop		r8
				pop		rbp
				pop		rsi
				pop		rdi
				pop		rdx
				pop		rcx
				pop		rbx
				pop		rax

				# Save kernel stack
				mov		gs:[8], rsp

				# ditto (1)
				je		2f
				swapgs
			2:

				rex64 iretq
			",
			options(noreturn),
			);
		}
	}

	pub fn set_sleep_until(&self, until: Monotonic) {
		self.sleep_until.set(until)
	}

	pub fn sleep_until(&self) -> Monotonic {
		self.sleep_until.get()
	}

	pub fn sleep(duration: Duration) {
		Self::current().set_sleep_until(Monotonic::now().saturating_add(duration));
		Self::yield_current();
	}

	pub fn yield_current() {
		crate::arch::yield_current_thread();
	}

	/// Cancel sleep
	pub fn wake(&self) {
		self.sleep_until.set(Monotonic::ZERO);
	}

	pub fn current() -> Arc<Self> {
		arch::amd64::current_thread()
	}

	pub fn current_weak() -> Weak<Self> {
		arch::amd64::current_thread_weak()
	}
}

impl Drop for Thread {
	fn drop(&mut self) {
		todo!()
	}
}
