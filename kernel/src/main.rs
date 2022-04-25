#![no_std]
#![no_main]
#![forbid(unused_must_use)]
#![feature(alloc_error_handler)]
#![feature(asm_const, asm_sym)]
#![feature(const_trait_impl, inline_const)]
#![feature(decl_macro)]
#![feature(derive_default_enum)]
#![feature(drain_filter)]
#![feature(let_else)]
#![feature(maybe_uninit_slice, maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(never_type)]
#![feature(new_uninit)]
#![feature(optimize_attribute)]
#![feature(slice_index_methods)]
#![feature(stmt_expr_attributes)]
#![feature(waker_getters)]
#![deny(incomplete_features)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use crate::memory::frame::{OwnedPageFrames, PageFrame, PageFrameIter, PPN};
use crate::memory::{frame::MemoryRegion, Page};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, sync::Arc};
use core::num::NonZeroUsize;
use core::panic::PanicInfo;

macro_rules! bi_from {
	(newtype $a:ident <=> $b:ident) => {
		impl From<$a> for $b {
			fn from(a: $a) -> $b {
				a.0
			}
		}

		impl From<$b> for $a {
			fn from(b: $b) -> $a {
				$a(b)
			}
		}
	};
}

#[macro_use]
mod log;

mod arch;
mod boot;
mod driver;
mod memory;
mod object_table;
mod scheduler;
mod sync;
mod time;
mod util;

#[export_name = "main"]
pub extern "C" fn main(boot_info: &boot::Info) -> ! {
	unsafe {
		driver::early_init(boot_info);
	}

	unsafe {
		log::init();
	}

	for region in boot_info.memory_regions() {
		let (base, size) = (region.base as usize, region.size as usize);
		let align = (Page::SIZE - base % Page::SIZE) % Page::SIZE;
		let base = base + align;
		let count = (size - align) / Page::SIZE;
		if let Ok(base) = PPN::try_from_usize(base) {
			let region = MemoryRegion { base, count };
			unsafe {
				memory::frame::add_memory_region(region);
			}
		}
	}

	unsafe {
		arch::init();
	}

	unsafe {
		driver::init(boot_info);
	}

	// TODO we should try to recuperate this memory when it becomes unused.
	struct Driver(&'static [u8]);

	impl MemoryObject for Driver {
		fn physical_pages(&self) -> Box<[PageFrame]> {
			let address = unsafe { memory::r#virtual::virt_to_phys(self.0.as_ptr()) };
			assert_eq!(
				address & u64::try_from(Page::MASK).unwrap(),
				0,
				"ELF file is not aligned"
			);
			let base = PPN((address >> Page::OFFSET_BITS).try_into().unwrap());
			let count = Page::min_pages_for_bytes(self.0.len());
			PageFrameIter { base, count }
				.map(|p| PageFrame { base: p, p2size: 0 })
				.collect()
		}
	}

	let drivers = boot_info
		.drivers()
		.map(|d| Arc::new(Driver(unsafe { d.as_slice() })))
		.collect::<alloc::vec::Vec<_>>();

	for program in boot_info.init_programs() {
		let driver = drivers[usize::from(program.driver())].clone();
		let mut stack = OwnedPageFrames::new(
			NonZeroUsize::new(1).unwrap(),
			memory::frame::AllocateHints {
				address: 0 as _,
				color: 0,
			},
		)
		.unwrap();
		unsafe {
			stack.clear();
		}
		unsafe {
			// args
			// Include driver name since basically every program ever expects that.
			let name = boot_info
				.drivers()
				.skip(usize::from(program.driver()))
				.next()
				.unwrap()
				.name();
			let count = (1 + program.args().count()).try_into().unwrap();

			let mut ptr = stack.physical_pages()[0].base.as_ptr().cast::<u8>();
			ptr.cast::<u16>().write(count);
			ptr = ptr.add(2);

			for s in [name].into_iter().chain(program.args()) {
				ptr.cast::<u16>().write(s.len().try_into().unwrap());
				ptr = ptr.add(2);
				ptr.copy_from_nonoverlapping(s.as_ptr(), s.len());
				ptr = ptr.add(s.len());
			}

			// env (should already be zero but meh, let's be clear)
			ptr.add(0).cast::<u16>().write(0);
		}
		match scheduler::process::Process::from_elf(driver, stack, 0) {
			Ok(_) => {} // We don't need to do anything.
			Err(e) => {
				error!("failed to start driver: {:?}", e)
			}
		}
	}

	// SAFETY: there is no thread state to save.
	unsafe { scheduler::next_thread() }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	fatal!("Panic!");
	fatal!("{:#?}", info);
	loop {
		arch::halt();
	}
}
