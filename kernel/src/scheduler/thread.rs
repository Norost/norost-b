#[cfg(not(target_has_atomic = "64"))]
use core::sync::atomic::AtomicU32;
#[cfg(target_has_atomic = "64")]
use core::sync::atomic::AtomicU64;
use {
	super::process::Process,
	crate::{
		arch,
		memory::{
			frame::{self, AllocateHints, OwnedPageFrames},
			r#virtual::{AddressSpace, RWX},
			Page,
		},
		sync::SpinLock,
		time::Monotonic,
	},
	alloc::{
		sync::{Arc, Weak},
		vec::Vec,
	},
	core::{
		arch::asm, cell::Cell, num::NonZeroUsize, ptr::NonNull, sync::atomic::Ordering, task::Waker,
	},
	norostb_kernel::Handle,
};

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
	/// How long this thread will sleep at minimum.
	///
	/// This also serves as the base for `wait`.
	///
	/// This field is only updated by the core running/holding this thread, so while
	/// synchronization is still necessary between threads (on the same core), there
	/// will never be concurrent writes to this location.
	///
	/// Hence, while a read may appear torn on some (32-bit) platforms, this will not be an
	/// issue in practice.
	/// Consider the following (extreme) scenario, where the value HL will be written to `sleep`:
	///
	/// * Thread writes 0 to 31:0 of `sleep`.
	/// * Thread is pre-empted.
	/// * Core checks `sleep`, finds that `sleep` is <= current time.
	///   * `sleep[63:32]` <= `time[63:32]`, since otherwise the thread wouldn't be running.
	///   * `sleep[31:0]` <= `time[31:0]`, as `sleep[31:0]` is 0.
	/// * Thread writes H to `sleep[63:32]`.
	/// * Thread is pre-empted.
	/// * Core checks `sleep`, finds that `sleep` is <= current time
	///   * If `sleep[63:32]` > `time[63:32]`, the thread will sleep and wake up sometime before
	///     HL, as `sleep[63:32]` is H and `sleep[31:0]` is 0.
	///   * If `sleep[63:32]` <= `time[63:32]`, the thread continues running.
	/// * Thread writes L to `sleep[31:0]`.
	/// * Thread yields.
	///
	/// So while a thread may wake too early, this is never visible to user-space applications.
	/// It will also never wake too late due to a torn write.
	/// Ergo, the current implementation will behave correctly.
	#[cfg(target_has_atomic = "64")]
	sleep: AtomicU64,
	#[cfg(not(target_has_atomic = "64"))]
	sleep: (AtomicU32, AtomicU32),
	/// How long this thread will wait for an event at most, in nanoseconds.
	/// This value is added to `sleep` to get the real wait time.
	///
	/// On platforms with 32-bit atomics this value is limited to 4 seconds.
	/// On platforms with 64-bit atomics it is limited to a few hundred years.
	///
	/// External events may set this to zero.
	#[cfg(target_has_atomic = "64")]
	wait: AtomicU64,
	#[cfg(not(target_has_atomic = "64"))]
	wait: AtomicU32,
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
				AllocateHints { address: 0 as _, color: 0 },
			)?;
			let (kernel_stack_base, _) =
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
				wait: Default::default(),
				arch_specific: arch::ThreadData::new_user(),
				waiters: Default::default(),
				destroyed: Cell::new(false),
			})
		}
	}

	#[cfg_attr(debug_assertions, track_caller)]
	#[inline]
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
	pub(super) fn kernel_new_0(
		start: extern "C" fn() -> !,
		enable_interrupts: bool,
	) -> Result<Self, frame::AllocateError> {
		Self::kernel_new(start as _, &[], enable_interrupts)
	}

	/// Create a new kernel-only thread.
	pub(super) fn kernel_new_1(
		start: extern "C" fn(usize) -> !,
		arg: usize,
		enable_interrupts: bool,
	) -> Result<Self, frame::AllocateError> {
		Self::kernel_new(start as _, &[arg], enable_interrupts)
	}

	/// Create a new kernel-only thread.
	fn kernel_new(
		start: *const (),
		args: &[usize],
		enable_interrupts: bool,
	) -> Result<Self, frame::AllocateError> {
		// TODO ditto
		unsafe {
			let kernel_stack_base = OwnedPageFrames::new(
				KERNEL_STACK_SIZE,
				AllocateHints { address: 0 as _, color: 0 },
			)?;
			let (kernel_stack_base, _) =
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

			// Set arguments
			debug_assert!(args.len() <= 1, "TODO: more arguments than 1");
			let (rdi, rsi) = (5, 6);
			for (&offt, &arg) in [rdi, rsi].iter().zip(args) {
				kernel_stack.sub(offt).write(arg);
			}

			// Reserve space for (zeroed) registers
			// 15 GP registers without RSP
			kernel_stack = kernel_stack.sub(15);

			Ok(Self {
				user_stack: Cell::new(None),
				kernel_stack: Cell::new(NonNull::new(kernel_stack).unwrap()),
				kernel_stack_base,
				process: None,
				sleep: Default::default(),
				wait: Default::default(),
				arch_specific: arch::ThreadData::new_kernel(),
				waiters: Default::default(),
				destroyed: Cell::new(false),
			})
		}
	}

	/// Get a reference to the owning process.
	#[cfg_attr(debug_assertions, track_caller)]
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

	/// When this thread should be woken up next.
	pub fn wake_time(&self) -> Monotonic {
		#[cfg(target_has_atomic = "64")]
		let t = self.sleep.load(Ordering::Relaxed);
		#[cfg(not(target_has_atomic = "64"))]
		let t = u64::from(self.sleep.0.load(Ordering::Relaxed))
			| u64::from(self.sleep.1.load(Ordering::Relaxed)) << 32;
		let t = Monotonic::from_nanos(t);
		t.saturating_add_nanos(self.wait.load(Ordering::Relaxed).into())
	}

	/// Sleep until the given time.
	pub fn sleep_until(&self, time: Monotonic) {
		if cfg!(debug_assertions) && time == Monotonic::MAX {
			warn!("thread will sleep forever");
		}
		self.wait_until(time, 0);
	}

	/// Wait for a given duration, using the given time as base.
	pub fn wait_until(&self, time: Monotonic, duration_ns: u64) {
		// Set wait first before setting sleep so the thread doesn't potentially sleep for too long.
		#[cfg(not(target_has_atomic = "64"))]
		let duration_ns = u32::try_from(duration_ns).unwrap_or(u32::MAX);
		self.wait.store(duration_ns, Ordering::Release);

		#[cfg(target_has_atomic = "64")]
		self.sleep.store(time.as_nanos(), Ordering::Relaxed);
		#[cfg(not(target_has_atomic = "64"))]
		{
			// See documentation of sleep member for explanation
			sleep.0.store(0, Ordering::Release);
			sleep
				.1
				.store((time.as_nanos() >> 32 as u32), Ordering::Release);
			sleep.0.store((time.as_nanos() as u32), Ordering::Relaxed);
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
				wake_thr.sleep_until(Monotonic::MAX);
			}
		}
		Ok(())
	}

	pub fn yield_current() {
		crate::arch::yield_current_thread();
	}

	/// Clear the wait value, waking this thread if the sleep duration has passed.
	pub fn wake(&self) {
		self.wait.store(0, Ordering::Relaxed);
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
