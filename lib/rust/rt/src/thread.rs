use alloc::boxed::Box;
use core::{mem, ptr, time::Duration};
use norostb_kernel::{error, syscall, Handle};

pub struct Thread(Handle);

impl Thread {
	/// Spawn a new thread.
	// FIXME determine if this should be unsafe
	// A current issue is the lack of catching panics, but that should be easy to fix once we
	// support unwinding.
	pub fn new(stack: usize, p: Box<dyn FnOnce()>) -> error::Result<Thread> {
		// All things that can fail will be handled before spawning so we don't need to wait
		// for the thread to confirm things are fine.

		// Allocate stack
		let (stack, stack_size) =
			syscall::alloc(None, stack, syscall::RWX::RW).map_err(|_| error::Error::Unknown)?;
		let stack = stack.cast::<u8>();

		// Allocate TLS
		let tls_ptr = match crate::tls::create_for_thread() {
			Ok(tls) => tls,
			Err(e) => {
				unsafe {
					syscall::dealloc(stack.cast(), stack_size.get()).unwrap();
				}
				return Err(e);
			}
		};

		// Push closure on the stack of the new thread
		let (ptr, meta) = Box::into_raw(p).to_raw_parts();
		let stack_top = stack
			.as_ptr()
			.wrapping_add(stack_size.get())
			.cast::<usize>();
		let mut stack_ptr = stack_top;
		let mut push = |v: usize| {
			stack_ptr = stack_ptr.wrapping_sub(1);
			// SAFETY: we will only push five usizes, which should fit well within a single
			// page.
			unsafe {
				stack_ptr.write(v);
			}
		};
		push(ptr as usize);
		push(unsafe { mem::transmute(meta) });
		push(stack.as_ptr() as usize);
		push(stack_size.get());
		push(tls_ptr as usize);

		unsafe extern "C" fn main(
			ptr: *mut (),
			meta: usize,
			stack_base: *const (),
			stack_size: usize,
			tls_ptr: *mut (),
		) -> ! {
			let meta = unsafe { mem::transmute(meta) };
			let p: Box<dyn FnOnce()> = unsafe { Box::from_raw(ptr::from_raw_parts_mut(ptr, meta)) };

			unsafe {
				super::tls::init_thread(tls_ptr);
			}

			p();

			unsafe {
				super::tls::deinit_thread();
			}

			// We're going to free the stack, so we need to resort to assembly
			unsafe {
				core::arch::asm!(
					// Deallocate stack
					"syscall",
					// Exit current thread
					"mov eax, {exit_thread}",
					"syscall",
					exit_thread = const syscall::ID_EXIT_THREAD,
					in("eax") syscall::ID_DEALLOC,
					in("rdi") stack_base,
					in("rsi") stack_size,
					options(noreturn, nostack),
				);
			}
		}

		#[naked]
		unsafe extern "C" fn start() -> ! {
			#[cfg(target_arch = "x86_64")]
			unsafe {
				core::arch::asm!(
					"mov rdi, [rsp - 8 * 1]",
					"mov rsi, [rsp - 8 * 2]",
					"mov rdx, [rsp - 8 * 3]",
					"mov rcx, [rsp - 8 * 4]",
					"mov r8, [rsp - 8 * 5]",
					// The stack must be 16-byte aligned *before* calling, so don't
					// use a jmp here.
					"call {main}",
					main = sym main,
					options(noreturn),
				);
			}
		}

		// Spawn thread
		unsafe {
			syscall::spawn_thread(start, stack_top as *const ())
				.map_err(|_| {
					syscall::dealloc(stack.cast(), stack_size.get()).unwrap();
					error::Error::Unknown
				})
				.map(Self)
		}
	}

	pub fn wait(self) {
		let _ = syscall::wait_thread(self.0);
	}
}

pub fn sleep(duration: Duration) {
	syscall::sleep(duration)
}

pub fn yield_now() {
	sleep(Duration::ZERO)
}
