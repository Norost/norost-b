use super::process::Process;
use crate::arch;
use crate::memory::{
	frame::{self, AllocateHints, OwnedPageFrames},
	r#virtual::{AddressSpace, RWX},
	Page,
};
use crate::sync::SpinLock;
use crate::time::Monotonic;
use alloc::{
	sync::{Arc, Weak},
	vec::Vec,
};
use core::arch::asm;
use core::cell::Cell;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::task::Waker;
use core::time::Duration;
use norostb_kernel::Handle;

const KERNEL_STACK_SIZE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(1) };

pub struct Thread {
	// TODO make these field non-pub and find out some other way to let
	// arch::amd64::syscall to access them.
	pub user_stack: Cell<Option<NonNull<usize>>>,
	pub kernel_stack: Cell<NonNull<usize>>,
	kernel_stack_base: NonNull<Page>,
	// This does create a cyclic reference and risks leaking memory, but should
	// be faster & more convienent in the long run.
	//
	// Especially, we don't need to store the processes anywhere as as long a thread
	// is alive, the process itself will be alive too.
	process: Option<Arc<Process>>,
	sleep: SpinLock<Sleep>,
	/// Architecture-specific data.
	pub arch_specific: arch::ThreadData,
	/// Tasks to notify when this thread finishes.
	waiters: SpinLock<Vec<Waker>>,
	destroyed: Cell<bool>,
}

impl Thread {
	pub fn new(
		start: usize,
		stack: usize,
		process: Arc<Process>,
	) -> Result<Self, frame::AllocateError> {
		// TODO move arch-specific code to crate::arch::amd64
		unsafe {
			let kernel_stack_base = OwnedPageFrames::new(
				KERNEL_STACK_SIZE,
				AllocateHints {
					address: 0 as _,
					color: 0,
				},
			)?;
			let kernel_stack_base =
				AddressSpace::kernel_map_object(None, Arc::new(kernel_stack_base), RWX::RW)
					.unwrap();
			let mut kernel_stack = kernel_stack_top(kernel_stack_base).cast::<usize>().as_ptr();
			let mut push = |val: usize| {
				kernel_stack = kernel_stack.sub(1);
				kernel_stack.write(val);
			};
			push(crate::arch::GDT::USER_SS.into());
			push(stack); // rsp
			push(0x202); // rflags: Set reserved bit 1, enable interrupts (IF)
			push(crate::arch::GDT::USER_CS.into());
			push(start); // rip

			// Push thread handle stub (rax)
			push(usize::MAX);

			// Reserve space for (zeroed) registers
			// 14 GP registers without RSP and RAX
			kernel_stack = kernel_stack.sub(14);

			Ok(Self {
				user_stack: Cell::new(NonNull::new(stack as *mut _)),
				kernel_stack: Cell::new(NonNull::new(kernel_stack).unwrap()),
				kernel_stack_base,
				process: Some(process),
				sleep: Default::default(),
				arch_specific: Default::default(),
				waiters: Default::default(),
				destroyed: Cell::new(false),
			})
		}
	}

	#[inline]
	#[track_caller]
	pub unsafe fn set_handle(&self, handle: Handle) {
		// Replace thread handle with proper value (rax)
		unsafe {
			self.kernel_stack
				.get()
				.cast::<usize>()
				.as_ptr()
				.add(14)
				.write(handle.try_into().unwrap());
		}
	}

	/// Create a new kernel-only thread.
	pub(super) fn kernel_new(
		start: extern "C" fn() -> !,
		enable_interrupts: bool,
	) -> Result<Self, frame::AllocateError> {
		// TODO ditto
		unsafe {
			let kernel_stack_base = OwnedPageFrames::new(
				KERNEL_STACK_SIZE,
				AllocateHints {
					address: 0 as _,
					color: 0,
				},
			)?;
			let kernel_stack_base =
				AddressSpace::kernel_map_object(None, Arc::new(kernel_stack_base), RWX::RW)
					.unwrap();
			// The stack must be aligned to 16 bytes *before* a call according to SysV ABI.
			// We will write 5 + 15 registers, which comes out at 160 bytes, ergo it is
			// already properly aligned.
			let mut kernel_stack = kernel_stack_top(kernel_stack_base).cast::<usize>().as_ptr();
			let stack = kernel_stack as usize;
			let mut push = |val: usize| {
				kernel_stack = kernel_stack.sub(1);
				kernel_stack.write(val);
			};
			push(crate::arch::amd64::GDT::KERNEL_SS.into());
			push(stack); // rsp
			 // rflags: Set reserved bit 1, enable interrupts (IF)
			push(0x2 | usize::from(enable_interrupts) * 0x200);
			push(crate::arch::amd64::GDT::KERNEL_CS.into());
			push(start as usize); // rip

			// Reserve space for (zeroed) registers
			// 15 GP registers without RSP
			kernel_stack = kernel_stack.sub(15);

			Ok(Self {
				user_stack: Cell::new(None),
				kernel_stack: Cell::new(NonNull::new(kernel_stack).unwrap()),
				kernel_stack_base,
				process: None,
				sleep: Default::default(),
				arch_specific: Default::default(),
				waiters: Default::default(),
				destroyed: Cell::new(false),
			})
		}
	}

	/// Get a reference to the owning process.
	#[track_caller]
	#[inline]
	pub fn process(&self) -> Option<&Arc<Process>> {
		self.process.as_ref()
	}

	/// Suspend the currently running thread & begin running this thread.
	///
	/// The thread may not have been destroyed already.
	pub fn resume(self: Arc<Self>) -> Result<!, Destroyed> {
		// TODO ditto
		if self.destroyed() {
			return Err(Destroyed);
		}

		unsafe {
			self.process.as_ref().map_or_else(
				|| AddressSpace::activate_default(),
				|a| a.activate_address_space(),
			);
		}

		// Ensure the kernel stack hasn't been corrupted
		#[cfg(debug_assertions)]
		unsafe {
			// 15 GP registers + RIP + CS + RFLAGS + RSP + SS
			let p = self.kernel_stack.get().as_ptr();
			match p
				.add(15 + 1)
				.read()
				.try_into()
				.expect("cs is not a 16 bit value")
			{
				crate::arch::amd64::GDT::KERNEL_CS => {
					assert_eq!(
						p.add(15 + 1 + 1 + 1 + 1).read(),
						crate::arch::amd64::GDT::KERNEL_SS.into(),
						"kernel stack is corrupted (ss mismatch)",
					)
				}
				// On return to userspace $rsp should be exactly equal to kernel_stack_top
				crate::arch::amd64::GDT::USER_CS => {
					assert_eq!(
						p.add(15 + 1 + 1 + 1 + 1 + 1),
						self.kernel_stack_top().as_ptr().cast(),
						"kernel stack is corrupted (rsp doesn't match kernel_stack_top)",
					);
					assert_eq!(
						p.add(15 + 1 + 1 + 1 + 1).read(),
						crate::arch::amd64::GDT::USER_SS.into(),
						"kernel stack is corrupted (ss mismatch)",
					)
				}
				cs => panic!("kernel stack is corrupted (cs is {:#x})", cs),
			}
		}

		unsafe {
			crate::arch::amd64::set_current_thread(self);
		}

		// iretq is the only way to preserve all registers
		unsafe {
			asm!(
				// Restore thread state
				"mov rsp, gs:[{kernel_stack}]",
				"pop r15",
				"pop r14",
				"pop r13",
				"pop r12",
				"pop r11",
				"pop r10",
				"pop r9",
				"pop r8",
				"pop rbp",
				"pop rsi",
				"pop rdi",
				"pop rdx",
				"pop rcx",
				"pop rbx",
				"pop rax",
				// Check if we need to swapgs by checking $cl
				"cmp DWORD PTR [rsp + 8], 8",
				"jz 2f",
				"swapgs",
				"2:",
				"iretq",
				kernel_stack = const crate::arch::CpuData::KERNEL_STACK_PTR,
				options(noreturn),
			);
		}
	}

	pub fn sleep_until(&self) -> Monotonic {
		self.sleep.auto_lock().until
	}

	/// Sleep until the given duration.
	///
	/// The thread may wake earlier if [`wake`] is called or if an asynchronous deadline is set.
	pub fn sleep(&self, duration: Duration) {
		{
			let mut s = self.sleep.lock();
			if core::mem::take(&mut s.waked) {
				// A task woke us. Poll it before yielding.
				return;
			}
			s.until = Monotonic::now().saturating_add(duration);
		}
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

		// SAFETY: The caller guarantees it is not using this stack.
		unsafe {
			AddressSpace::kernel_unmap_object(self.kernel_stack_base, KERNEL_STACK_SIZE).unwrap();
		}

		self.destroyed.set(true);

		for w in self.waiters.auto_lock().drain(..) {
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
		let mut s = self.sleep.auto_lock();
		s.until = Monotonic::ZERO;
		s.waked = true;
	}

	pub fn current() -> Option<Arc<Self>> {
		arch::amd64::current_thread()
	}

	pub fn current_weak() -> Option<Weak<Self>> {
		arch::amd64::current_thread_weak()
	}

	#[inline(always)]
	pub fn current_ptr() -> Option<NonNull<Self>> {
		arch::amd64::current_thread_ptr()
	}

	#[inline]
	pub fn kernel_stack_top(&self) -> NonNull<Page> {
		kernel_stack_top(self.kernel_stack_base)
	}

	/// Whether this thread has been destroyed.
	pub fn destroyed(&self) -> bool {
		self.destroyed.get()
	}
}

fn kernel_stack_top(base: NonNull<Page>) -> NonNull<Page> {
	unsafe { NonNull::new_unchecked(base.as_ptr().add(KERNEL_STACK_SIZE.get())) }
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

#[derive(Default)]
struct Sleep {
	/// How long to wait.
	until: Monotonic,
	/// Whether a task has sent a wake-up event to this thread. If set, [`Thread::sleep`] will
	/// clear this flag and return immediately.
	waked: bool,
}
