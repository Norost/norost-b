mod memory_object;
pub mod process;
pub mod syscall;
mod thread;
mod round_robin;

pub use memory_object::*;
pub use thread::Thread;

pub use round_robin::count as thread_count;

/// Switch to the next thread. This does save the current thread's state!
///
/// # Safety
///
/// The current thread's state must be properly saved.
pub unsafe fn next_thread() -> ! {
	let thr = round_robin::next().unwrap();
	thr.resume();
}
