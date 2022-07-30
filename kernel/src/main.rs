#![no_std]
#![no_main]
#![forbid(unused_must_use)]
#![feature(alloc_error_handler)]
#![feature(asm_const, asm_sym)]
#![feature(
	const_btree_new,
	const_default_impls,
	const_maybe_uninit_uninit_array,
	const_trait_impl,
	inline_const
)]
#![feature(decl_macro)]
#![feature(drain_filter)]
#![feature(if_let_guard, let_else)]
#![feature(maybe_uninit_slice, maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(never_type)]
#![feature(new_uninit)]
#![feature(optimize_attribute)]
#![feature(pointer_byte_offsets, pointer_is_aligned)]
#![feature(result_flattening)]
#![feature(slice_index_methods)]
#![feature(stmt_expr_attributes)]
#![feature(waker_getters)]
#![feature(bench_black_box)]
#![deny(incomplete_features)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_variables)]

extern crate alloc;

use crate::object_table::{Error, Object};
use alloc::sync::{Arc, Weak};
use core::panic::PanicInfo;

#[macro_use]
mod log;

mod arch;
mod boot;
mod driver;
mod initfs;
mod memory;
mod object_table;
mod scheduler;
mod sync;
mod time;
mod util;

#[export_name = "main"]
pub extern "C" fn main(boot_info: &'static mut boot::Info) -> ! {
	unsafe {
		driver::early_init(boot_info);
	}

	unsafe {
		memory::init(boot_info.memory_regions_mut());
		arch::init();
		driver::init(boot_info);
		scheduler::init();
	}

	scheduler::new_kernel_thread_1(post_init, boot_info as *mut _ as _, true)
		.expect("failed to spawn thread for post-initialization");

	// SAFETY: there is no thread state to save.
	unsafe { scheduler::next_thread() }
}

/// A kernel thread that handles the rest of the initialization.
///
/// Mutexes may be used here as interrupts are enabled at this point.
extern "C" fn post_init(boot_info: usize) -> ! {
	let boot_info = unsafe { &mut *(boot_info as *mut boot::Info) };
	let root = Arc::new(object_table::Root::new());

	memory::post_init(&root);
	driver::post_init(&root);
	scheduler::post_init(&root);
	log::post_init(&root);
	let fs = initfs::post_init(boot_info);

	let init = fs.find(b"init").expect("no init has been specified");
	root.add(*b"drivers", Arc::downgrade(&fs) as Weak<dyn Object>);
	let _ = Arc::into_raw(fs); // Make sure FS object stays alive.

	// Spawn init
	let mut objects = arena::Arena::<Arc<dyn Object>, _>::new();
	objects.insert(root);
	scheduler::process::Process::from_elf(init, None, 0, objects).expect("failed to spawn init");

	scheduler::exit_kernel_thread()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	arch::disable_interrupts();
	fatal!("Panic!");
	fatal!("{}", info);
	loop {
		arch::halt();
	}
}
