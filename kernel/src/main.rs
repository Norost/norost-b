#![no_std]
#![no_main]
#![forbid(unused_must_use)]
#![feature(alloc_error_handler)]
#![feature(asm_const, asm_sym)]
#![feature(const_btree_new, const_fn_trait_bound, const_trait_impl, inline_const)]
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
use crate::object_table::{Error, NoneQuery, Object, OneQuery, Query, QueryIter, Ticket};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::cell::Cell;
use core::mem::ManuallyDrop;
use core::num::NonZeroUsize;
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

	let root = Arc::new(object_table::Root::new());

	unsafe {
		driver::init(boot_info, &root);
	}

	scheduler::init(&root);

	let drivers = boot_info
		.drivers()
		.inspect(|d| {
			// SAFETY: only one thread is running at the moment and there are no other
			// mutable references to DRIVERS.
			unsafe {
				DRIVERS.insert(d.name().into(), Driver(d.as_slice()));
			}
		})
		.map(|d| Arc::new(Driver(unsafe { d.as_slice() })))
		.collect::<alloc::vec::Vec<_>>();

	let stdout = Arc::new(driver::uart::UartId::new(0));

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
			let ptr = stack.physical_pages()[0].base.as_ptr().cast::<u8>();

			// handles
			let mut ptr = ptr.cast::<u32>();
			let mut f = |n| {
				ptr.write(n);
				ptr = ptr.add(1);
			};
			f(4);
			// FIXME don't hardcode this.
			f(0);
			f(0x00_000000);
			f(1);
			f(0x01_000001);
			f(2);
			f(0x02_000002);
			f(3);
			f(0x03_000003);
			let mut ptr = ptr.cast::<u8>();

			// args
			// Include driver name since basically every program ever expects that.
			let name = boot_info
				.drivers()
				.skip(usize::from(program.driver()))
				.next()
				.unwrap()
				.name();
			let count = (1 + program.args().count()).try_into().unwrap();

			ptr.cast::<u16>().write_unaligned(count);
			ptr = ptr.add(2);

			for s in [name].into_iter().chain(program.args()) {
				ptr.cast::<u16>()
					.write_unaligned(s.len().try_into().unwrap());
				ptr = ptr.add(2);
				ptr.copy_from_nonoverlapping(s.as_ptr(), s.len());
				ptr = ptr.add(s.len());
			}

			// env (should already be zero but meh, let's be clear)
			ptr.add(0).cast::<u16>().write_unaligned(0);
		}
		let mut objects = arena::Arena::<Arc<dyn Object>, _>::new();
		objects.insert(stdout.clone());
		objects.insert(stdout.clone());
		objects.insert(stdout.clone());
		objects.insert(root.clone());
		match scheduler::process::Process::from_elf(driver, stack, 0, objects) {
			Ok(_) => {}
			Err(e) => {
				error!("failed to start driver: {:?}", e)
			}
		}
	}

	root.add(&b"drivers"[..], {
		Arc::downgrade(&ManuallyDrop::new(Arc::new(Drivers) as Arc<dyn Object>))
	});

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

/// A single driver binary.
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

struct DriverObject {
	data: &'static [u8],
	position: Cell<usize>,
}

impl Object for DriverObject {
	fn read(&self, _: u64, length: usize) -> Ticket<Box<[u8]>> {
		let bottom = self.data.len().min(self.position.get());
		let top = self.data.len().min(self.position.get() + length);
		self.position.set(top);
		Ticket::new_complete(Ok(self.data[bottom..top].into()))
	}

	fn peek(&self, _: u64, length: usize) -> Ticket<Box<[u8]>> {
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

	fn memory_object(&self, _: u64) -> Option<Box<dyn MemoryObject>> {
		Some(Box::new(Driver(self.data)))
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
	fn query(self: Arc<Self>, prefix: Vec<u8>, path: &[u8]) -> Ticket<Box<dyn Query>> {
		Ticket::new_complete(Ok(if path == b"" {
			let it = unsafe {
				DRIVERS.keys().map(move |s| {
					let mut v = prefix.clone();
					v.extend(&**s);
					v
				})
			};
			Box::new(QueryIter::new(it))
		} else if drivers().contains_key(path) {
			Box::new(OneQuery {
				path: Some(path.into()),
			})
		} else {
			Box::new(NoneQuery)
		}))
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(drivers().get(path).map_or(Err(Error::DoesNotExist), |d| {
			Ok(Arc::new(DriverObject {
				data: d.0,
				position: Cell::new(0),
			}))
		}))
	}
}
