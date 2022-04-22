use super::process::Process;
use crate::arch;
use crate::memory::{
	frame::{self, PageFrame, PPN},
	Page,
};
use crate::sync::Mutex;
use crate::time::Monotonic;
use alloc::{
	sync::{Arc, Weak},
	vec::Vec,
};
use core::arch::asm;
use core::cell::Cell;
use core::ptr::NonNull;
use core::task::{Context, Waker};
use core::time::Duration;
use norostb_kernel::Handle;

pub struct Thread {
	pub user_stack: Cell<Option<NonNull<usize>>>,
	pub kernel_stack: Cell<Option<NonNull<usize>>>,
	// This does create a cyclic reference and risks leaking memory, but should
	// be faster & more convienent in the long run.
	//
	// Especially, we don't need to store the processes anywhere as as long a thread
	// is alive, the process itself will be alive too.
	process: Arc<Process>,
	sleep_until: Cell<Monotonic>,
	/// Async deadline set by [`super::waker::sleep`].
	async_deadline: Cell<Option<Monotonic>>,
	/// Architecture-specific data.
	pub arch_specific: arch::ThreadData,
	/// Tasks to notify when this thread finishes.
	waiters: Mutex<Vec<Waker>>,
}

impl Thread {
	pub fn new(
		start: usize,
		stack: usize,
		process: Arc<Process>,
		handle: Handle,
	) -> Result<Self, frame::AllocateContiguousError> {
		unsafe {
			let mut kernel_stack_base = None;
			frame::allocate(1, |f| kernel_stack_base = Some(f), 0 as _, 0).unwrap();
			let kernel_stack_base = kernel_stack_base.unwrap().base;
			let kernel_stack_base = kernel_stack_base.as_ptr().cast::<[usize; 512]>();
			let mut kernel_stack = kernel_stack_base.add(1).cast::<usize>();
			let mut push = |val: usize| {
				kernel_stack = kernel_stack.sub(1);
				kernel_stack.write(val);
			};
			push(4 * 8 | 3); // ss
			push(stack); // rsp
			 //push(0x202);     // rflags: Set reserved bit 1, enable interrupts (IF)
			push(0x2); // rflags: Set reserved bit 1
			push(3 * 8 | 3); // cs
			push(start); // rip

			// Push thread handle (rax)
			push(handle.try_into().unwrap());

			// Reserve space for (zeroed) registers
			// 14 GP registers without RSP and RAX
			// FIXME save RFLAGS
			kernel_stack = kernel_stack.sub(14);

			Ok(Self {
				user_stack: Cell::new(NonNull::new(stack as *mut _)),
				kernel_stack: Cell::new(Some(NonNull::new(kernel_stack).unwrap())),
				process,
				sleep_until: Cell::new(Monotonic::ZERO),
				async_deadline: Cell::new(None),
				arch_specific: Default::default(),
				waiters: Default::default(),
			})
		}
	}

	/// Get a reference to the owning process.
	pub fn process(&self) -> &Arc<Process> {
		&self.process
	}

	/// Async deadline set by [`super::waker::sleep`].
	pub(super) fn set_async_deadline(&self, time: Monotonic) {
		self.async_deadline.set(Some(time));
	}

	/// Suspend the currently running thread & begin running this thread.
	///
	/// The thread may not have been destroyed already.
	pub fn resume(self: Arc<Self>) -> Result<!, Destroyed> {
		if self.destroyed() {
			return Err(Destroyed);
		}

		unsafe { self.process.as_ref().activate_address_space() };

		unsafe {
			crate::arch::amd64::set_current_thread(self);
		}

		// iretq is the only way to preserve all registers
		unsafe {
			asm!(
				"
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

	/// Sleep until the given duration.
	///
	/// The thread may wake earlier if [`wake`] is called or if an asynchronous deadline is set.
	pub fn sleep(&self, duration: Duration) {
		let t = self
			.async_deadline
			.replace(None)
			.unwrap_or_else(|| Monotonic::now().saturating_add(duration));
		// TODO huh? Why Self::current()?
		Self::current().unwrap().set_sleep_until(t);
		Self::yield_current();
	}

	/// Destroy this thread.
	///
	/// # Safety
	///
	/// No CPU may be using *any* resource of this thread, especially the stack.
	pub unsafe fn destroy(self: Arc<Self>) {
		// The kernel stack is convienently exactly one page large, so masking the lower bits
		// will give us the base of the frame.

		// FIXME ensure the thread has been stopped.

		// SAFETY: the kernel pointer should be valid lest we smashed the stack.
		// The caller also guaranteed it is not using this stack.
		unsafe {
			let kernel_stack = self.kernel_stack.take().unwrap().as_ptr() as usize & !Page::MASK;
			let kernel_stack = PPN::from_ptr(kernel_stack as *mut _);
			frame::deallocate(1, || PageFrame {
				base: kernel_stack,
				p2size: 0,
			})
			.unwrap();
		}

		for w in self.waiters.lock().drain(..) {
			w.wake();
		}
	}

	/// Wait for this thread to finish. Waiting is only possible if the caller is inside an
	/// active thread.
	pub fn wait(&self) -> Result<(), ()> {
		let mut waiters = self.waiters.lock();
		if !self.destroyed() {
			let wake_thr = Thread::current().ok_or(())?;
			let waker = super::waker::new_waker(Arc::downgrade(&wake_thr));
			waiters.push(waker);
			drop(waiters);
			while !self.destroyed() {
				wake_thr.sleep(Duration::MAX);
			}
		}
		Ok(())
	}

	pub fn yield_current() {
		crate::arch::yield_current_thread();
	}

	/// Cancel sleep
	pub fn wake(&self) {
		self.sleep_until.set(Monotonic::ZERO);
	}

	pub fn current() -> Option<Arc<Self>> {
		arch::amd64::current_thread()
	}

	pub fn current_weak() -> Option<Weak<Self>> {
		arch::amd64::current_thread_weak()
	}

	/// Whether this thread has been destroyed.
	pub fn destroyed(&self) -> bool {
		self.kernel_stack.get().is_none()
	}
}

impl Drop for Thread {
	fn drop(&mut self) {
		// We currently cannot destroy a thread in a safe way but we also need to ensure
		// resources are cleaned up properly, so do log it for debugging potential leaks at least.
		debug!("cleaning up thread");
	}
}

#[derive(Debug)]
pub struct Destroyed;
