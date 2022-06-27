#![no_std]
#![no_main]
#![forbid(unused_must_use)]
#![feature(alloc_error_handler)]
#![feature(asm_const, asm_sym)]
#![feature(
	const_btree_new,
	const_maybe_uninit_uninit_array,
	const_trait_impl,
	inline_const
)]
#![feature(decl_macro)]
#![feature(drain_filter)]
#![feature(let_else)]
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

use crate::{
	memory::{
		frame::{PageFrameIter, PPN},
		Page,
	},
	object_table::{Error, MemoryObject, Object, QueryIter, Ticket},
};
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use core::{cell::Cell, mem::ManuallyDrop, panic::PanicInfo};

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
pub extern "C" fn main(boot_info: &'static mut boot::Info) -> ! {
	dbg!(boot_info as *const _);
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
	dbg!(boot_info as *const ());
	let boot_info = unsafe { &mut *(boot_info as *mut boot::Info) };
	let root = Arc::new(object_table::Root::new());

	// TODO anything involving a root object should be moved to post_init
	memory::post_init(&root);
	driver::post_init(&root);
	scheduler::post_init(&root);
	log::post_init(&root);

	let mut init = None;
	for d in boot_info.drivers() {
		// SAFETY: only one thread is running at the moment and there are no other
		// mutable references to DRIVERS.
		// FIXME we can make this entirely safe by creating the drivers map here.
		unsafe {
			DRIVERS.insert(d.name().into(), Driver(d.as_slice()));
			if d.name() == b"init" {
				assert!(init.is_none(), "init has already been set");
				init = Some(Driver(d.as_slice()));
			}
		}
	}
	let init = init.expect("no init has been specified");

	root.add(&b"drivers"[..], {
		Arc::downgrade(&ManuallyDrop::new(Arc::new(Drivers) as Arc<dyn Object>))
	});

	// Spawn init
	let mut objects = arena::Arena::<Arc<dyn Object>, _>::new();
	objects.insert(root);
	scheduler::process::Process::from_elf(Arc::new(init), None, 0, objects)
		.expect("failed to spawn init");

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

/// A single driver binary.
struct Driver(&'static [u8]);

unsafe impl MemoryObject for Driver {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		let address = unsafe { memory::r#virtual::virt_to_phys(self.0.as_ptr()) };
		assert_eq!(
			address & u64::try_from(Page::MASK).unwrap(),
			0,
			"ELF file is not aligned"
		);
		let base = PPN((address >> Page::OFFSET_BITS).try_into().unwrap());
		let count = Page::min_pages_for_bytes(self.0.len());
		for p in (PageFrameIter { base, count }) {
			if !f(&[p]) {
				break;
			}
		}
	}

	fn physical_pages_len(&self) -> usize {
		Page::min_pages_for_bytes(self.0.len())
	}
}

struct DriverObject {
	data: &'static [u8],
	position: Cell<usize>,
}

impl Object for DriverObject {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		let bottom = self.data.len().min(self.position.get());
		let top = self.data.len().min(self.position.get() + length);
		self.position.set(top);
		Ticket::new_complete(Ok(self.data[bottom..top].into()))
	}

	fn peek(&self, length: usize) -> Ticket<Box<[u8]>> {
		let bottom = self.data.len().min(self.position.get());
		let top = self.data.len().min(self.position.get() + length);
		Ticket::new_complete(Ok(self.data[bottom..top].into()))
	}

	fn seek(&self, from: norostb_kernel::io::SeekFrom) -> Ticket<u64> {
		self.position.set(match from {
			norostb_kernel::io::SeekFrom::Start(n) => n.try_into().unwrap_or(usize::MAX),
			norostb_kernel::io::SeekFrom::Current(n) => (i64::try_from(self.position.get())
				.unwrap() + n)
				.try_into()
				.unwrap(),
			norostb_kernel::io::SeekFrom::End(n) => (i64::try_from(self.data.len()).unwrap() + n)
				.try_into()
				.unwrap(),
		});
		Ticket::new_complete(Ok(self.position.get().try_into().unwrap()))
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(Arc::new(Driver(self.data)))
	}
}

/// A list of all drivers.
static mut DRIVERS: BTreeMap<Box<[u8]>, Driver> = BTreeMap::new();

fn drivers() -> &'static BTreeMap<Box<[u8]>, Driver> {
	// SAFETY: DRIVERS is set once in main and never again after.
	unsafe { &DRIVERS }
}

/// A table with all driver binaries. This is useful for restarting drivers if necessary.
struct Drivers;

impl Object for Drivers {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if path == b"" {
			Ok(Arc::new(QueryIter::new(
				drivers().keys().map(|s| s.to_vec()),
			)))
		} else {
			drivers().get(path).map_or(Err(Error::DoesNotExist), |d| {
				Ok(Arc::new(DriverObject {
					data: d.0,
					position: Cell::new(0),
				}))
			})
		})
	}
}
