mod memory_object;
pub mod process;
mod round_robin;
pub mod syscall;
mod thread;
mod waker;

use crate::{
	arch, driver::apic, memory::frame::AllocateError, object_table::Root, time::Monotonic,
};
use alloc::sync::Arc;
use core::future::Future;
use core::marker::Unpin;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;
pub use memory_object::*;
pub use thread::Thread;

static mut SLEEP_THREADS: [MaybeUninit<Arc<Thread>>; 1] = MaybeUninit::uninit_array();

const TIME_SLICE: Duration = Duration::from_millis(33); // 30 times / sec

/// Switch to the next thread. This does not save the current thread's state!
///
/// If no thread is scheduled, the `Monotonic` **when** the next thread becomes available is
/// returned.
///
/// # Safety
///
/// The current thread's state must be properly saved.
#[track_caller]
unsafe fn try_next_thread() -> Result<!, Monotonic> {
	let mut thr = round_robin::next().unwrap();
	let first = Arc::as_ptr(&thr);
	let now = Monotonic::now();
	let mut t = Monotonic::MAX;
	loop {
		let sleep_until = thr.sleep_until();
		if sleep_until <= now {
			// Be very careful _not_ to clone here, as otherwise we'll start leaking references.
			apic::set_timer_oneshot(TIME_SLICE);
			let _ = thr.resume();
		}
		t = t.min(sleep_until);
		thr = round_robin::next().unwrap();
		if Arc::as_ptr(&thr) == first {
			return Err(t);
		}
	}
}

/// Switch to the next thread. This does not save the current thread's state!
///
/// # Safety
///
/// The current thread's state must be properly saved.
pub unsafe fn next_thread() -> ! {
	loop {
		if let Err(t) = unsafe { try_next_thread() } {
			let now = Monotonic::now();
			if let Some(d) = now.duration_until(t) {
				debug!("{:?} <- {:?} {:?}", d, now, t);
				apic::set_timer_oneshot(d);
				unsafe {
					SLEEP_THREADS[0].assume_init_ref().clone().resume().unwrap();
				}
			}
		}
	}
}

/// Poll a task once.
fn poll<T>(mut task: impl Future<Output = T> + Unpin) -> Poll<T> {
	let waker = waker::new_waker(Thread::current_weak().unwrap());
	Pin::new(&mut task).poll(&mut Context::from_waker(&waker))
}

/// Block on a task until it finishes.
fn block_on<T>(mut task: impl Future<Output = T> + Unpin) -> T {
	let waker = waker::new_waker(Thread::current_weak().unwrap());
	let mut cx = Context::from_waker(&waker);
	loop {
		match Pin::new(&mut task).poll(&mut cx) {
			Poll::Ready(t) => return t,
			Poll::Pending => Thread::current().unwrap().sleep(Duration::MAX),
		}
	}
}

/// Spawn a new kernel thread.
pub fn new_kernel_thread_1(
	f: extern "C" fn(usize) -> !,
	arg: usize,
	enable_interrupts: bool,
) -> Result<(), AllocateError> {
	let thr = Arc::new(Thread::kernel_new_1(f, arg, enable_interrupts)?);
	round_robin::insert(Arc::downgrade(&thr));
	// Forget about the thread so the scheduler can actually do something with it.
	let _ = Arc::into_raw(thr);
	Ok(())
}

/// Exit a kernel thread.
pub fn exit_kernel_thread() -> ! {
	// We already leaked a strong reference, so we must not increment it further.
	let d = Thread::current_ptr().unwrap().as_ptr();
	arch::run_on_local_cpu_stack_noreturn!(exit, d as *const ());

	extern "C" fn exit(data: *const ()) -> ! {
		// SAFETY: we leaked a strong reference in new_kernel_thread_*
		let thread = unsafe { Arc::from_raw(data.cast::<Thread>()) };
		arch::amd64::clear_current_thread();
		// SAFETY: we switched to the CPU local stack and won't return to the stack of this thread
		// We also switched to the default address space in case it's the last thread of the
		// process.
		unsafe {
			thread.destroy();
		}
		// SAFETY: there is no thread state to save.
		unsafe { next_thread() }
	}
}

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init() {
	unsafe {
		for t in SLEEP_THREADS.iter_mut() {
			t.write(
				Thread::kernel_new_0(arch::scheduler::halt_forever, false)
					.expect("failed to create sleep thread")
					.into(),
			);
		}
	}
}

pub fn post_init(root: &Root) {
	process::post_init(root);
}
