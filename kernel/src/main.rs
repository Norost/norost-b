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
#![feature(slice_index_methods)]
#![feature(stmt_expr_attributes)]
#![feature(waker_getters)]
#![feature(bench_black_box)]
#![deny(incomplete_features)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use crate::memory::frame::{PageFrameIter, PPN};
use crate::memory::{frame::MemoryRegion, Page};
use crate::object_table::{Error, Object, QueryIter, Ticket};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};
use core::cell::Cell;
use core::mem::ManuallyDrop;
use core::panic::PanicInfo;

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

	let root = Arc::new(object_table::Root::new());

	unsafe {
		driver::init(boot_info, &root);
	}

	unsafe {
		log::post_init(&root);
	}

	unsafe {
		scheduler::init(&root);
	}

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
	match scheduler::process::Process::from_elf(Arc::new(init), None, 0, objects) {
		Ok(_) => {}
		Err(e) => {
			error!("failed to start driver: {:?}", e)
		}
	}

	// SAFETY: there is no thread state to save.
	unsafe { scheduler::next_thread() }
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
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN])) {
		let address = unsafe { memory::r#virtual::virt_to_phys(self.0.as_ptr()) };
		assert_eq!(
			address & u64::try_from(Page::MASK).unwrap(),
			0,
			"ELF file is not aligned"
		);
		let base = PPN((address >> Page::OFFSET_BITS).try_into().unwrap());
		let count = Page::min_pages_for_bytes(self.0.len());
		PageFrameIter { base, count }.for_each(|p| f(&[p]));
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

	fn memory_object(self: Arc<Self>, _: u64) -> Option<Arc<dyn MemoryObject>> {
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
