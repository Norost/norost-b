use crate::memory::frame;

#[repr(C)]
pub struct Thread {
	kernel_stack_base: *mut [usize; 512],
	kernel_stack: *mut usize,
}

impl Thread {
	pub fn new(start: usize) -> Result<Self, frame::AllocateContiguousError> {
		unsafe {
			let kernel_stack_base = frame::allocate_contiguous(1)?.as_ptr().cast::<[usize; 512]>();
			let mut kernel_stack = kernel_stack_base.add(1).cast::<usize>();
			let mut push = |val: usize| {
				kernel_stack = kernel_stack.sub(1);
				kernel_stack.write(val);
			};
			push(4 * 8 | 3); // ss
			push(0);         // rsp
			//push(0x202);     // rflags: Set reserved bit 1, enable interrupts (IF)
			push(0x2);       // rflags: Set reserved bit 1
			push(3 * 8 | 3); // cs
			push(start);     // rip
			// Reserve space for (zeroed) registers
			kernel_stack = kernel_stack.sub(16);
			Ok(Self { kernel_stack_base, kernel_stack })
		}
	}

	pub fn resume(&self) -> ! {
		// iretq is the only way to preserve all registers
		unsafe {
			asm!("
				# Set kernel stack
				mov		rsp, {0}

				mov		ax, (4 * 8) | 3		# ring 3 data with bottom 2 bits set for ring 3
				mov		ds, ax
				mov 	es, ax

				pop		rax
				pop		rbx
				pop		rcx
				pop		rdx
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

				mov		gs:[8], rsp
				swapgs

				rex64 iretq
			", in(reg) self.kernel_stack, options(noreturn));
		}
	}
}
