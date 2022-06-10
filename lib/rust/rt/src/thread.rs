#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use crate::time::Monotonic;
use alloc::boxed::Box;
use core::{mem, ptr, time::Duration};
use norostb_kernel::{error, syscall, Handle};

pub struct Thread(Handle);

impl Thread {
	/// Spawn a new thread.
	pub unsafe fn new(stack: usize, p: Box<dyn FnOnce()>) -> error::Result<Thread> {
		// All things that can fail will be handled before spawning so we don't need to wait
		// for the thread to confirm things are fine.

		// Allocate stack
		let (stack, stack_size) =
			syscall::alloc(None, stack, syscall::RWX::RW).map_err(|_| error::Error::Unknown)?;
		let stack = stack.cast::<u8>();

		// Allocate TLS
		let tls_ptr = match crate::io::create_for_thread() {
			Ok(queue) => match crate::tls::create_for_thread(queue) {
				Ok(tls) => tls,
				Err(e) => {
					// TODO free queue
					unsafe {
						syscall::dealloc(stack.cast(), stack_size.get(), false, false).unwrap();
					}
					return Err(e);
				}
			},
			Err(e) => {
				unsafe {
					syscall::dealloc(stack.cast(), stack_size.get(), false, false).unwrap();
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
			handle: Handle,
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
					// Kill current thread
					"mov eax, {kill_thread}",
					"mov rdi, r12",
					"syscall",
					kill_thread = const syscall::ID_KILL_THREAD,
					in("eax") syscall::ID_DEALLOC,
					in("rdi") stack_base,
					in("rsi") stack_size,
					in("rdx") 0,
					// Rust is retarded and doesn't let us specify clobbers with out
					// so we have to avoid rax, rdx, rcx and r11 manually *sigh*
					in("r12") handle,
					options(noreturn, nostack),
				);
			}
		}

		#[naked]
		unsafe extern "C" fn start() -> ! {
			#[cfg(target_arch = "x86_64")]
			unsafe {
				core::arch::asm!("
					mov rdi, [rsp - 8 * 1]
					mov rsi, [rsp - 8 * 2]
					mov rdx, [rsp - 8 * 3]
					mov rcx, [rsp - 8 * 4]
                    mov r9, [rsp - 8 * 5]
					mov r8, rax
					jmp {main}
					",
					main = sym main,
					options(noreturn),
				);
			}
		}

		// Spawn thread
		unsafe {
			syscall::spawn_thread(start, stack_top as *const ())
				.map_err(|_| {
					syscall::dealloc(stack.cast(), stack_size.get(), false, false).unwrap();
					error::Error::Unknown
				})
				.map(Self)
		}
	}

	pub fn wait(self) {
		let _ = syscall::wait_thread(self.0);
	}
}

pub fn sleep(duration: Duration) -> Monotonic {
	syscall::sleep(duration)
}

pub fn yield_now() -> Monotonic {
	sleep(Duration::ZERO)
}
