#![no_main]
#![no_std]
#![feature(asm)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_slice)]
#![feature(option_result_unwrap_unchecked)]

mod cpuid;
mod elf64;
mod gdt;
mod info;
mod multiboot2;
mod msr;
mod mtrr;
mod paging;
mod vga;

use core::convert::TryInto;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::slice;

#[link_section = ".init.gdt"]
static GDT: gdt::GDT = gdt::GDT::new();
static GDT_PTR: gdt::GDTPointer = gdt::GDTPointer::new(&GDT);

extern "C" {
	static boot_top: usize;
	static boot_bottom: usize;
}

#[repr(C)]
struct Return {
	entry: u64,
	pml4: &'static paging::PML4,
	info: &'static info::Info,
}

static mut INFO: info::Info = info::Info::empty();
static mut VGA: Option<vga::Text> = None;

#[export_name = "main"]
extern "fastcall" fn main(magic: u32, arg: *const u8) -> Return {
	unsafe {
		VGA = Some(vga::Text::new());
	}

	let cpuid = cpuid::Features::new().unwrap_or_else(|| {
		print_err(b"No CPUID support");
		halt();
	});

	if magic != 0x36d76289 {
		print_err(b"Bad multiboot2 magic: ");
		print_err_num(magic.into(), 16);
		halt();
	}

	use multiboot2::bootinfo as bi;

	let mut avail_memory = MaybeUninit::uninit_array::<8>();
	let mut avail_memory_count = 0;
	let mut kernel = None;
	let mut drivers = MaybeUninit::uninit_array::<32>();
	let mut drivers_count = 0;

	let (bt_top, bt_bottom) = unsafe {
		(&boot_top as *const _ as usize, &boot_bottom as *const _ as usize)
	};

	for e in unsafe { bi::BootInfo::new(arg) } {
		match e {
			bi::Info::Unknown(_) => (),
			bi::Info::Module(m) => {
				match m.string {
					b"kernel" => kernel = Some(m),
					b"driver" => {
						let d = info::Driver {
							address: m.start as u32,
							size: (m.end - m.start) as u32,
						};
						drivers[drivers_count].write(d);
						drivers_count += 1;
					}
					m => {
						print_err(b"Unknown module type: ");
						print_err(m);
						halt();
					},
				}
			}
			bi::Info::MemoryMap(m) => {
				m.entries.iter().filter(|e| e.is_available()).for_each(|e| {
					let (mut base, mut size) = (e.base_address, e.length);
					if e.base_address == 0 {
						// Split of the first page so we can avoid writing to null (which is UB)
						avail_memory[avail_memory_count].write(info::MemoryRegion {
							base: 0,
							size: 4096,
						});
						avail_memory_count += 1;
						base += 4096;
						size -= 4096;
					}
					avail_memory[avail_memory_count].write(info::MemoryRegion {
						base,
						size,
					});
					avail_memory_count += 1;
				});
			}
		}
	}

	let kernel = kernel.unwrap_or_else(|| {
		print_err(b"No kernel module");
		halt();
	});

	let drivers = unsafe {
		MaybeUninit::slice_assume_init_ref(&drivers[..drivers_count])
	};

	// Determine (guess) the maximum valid physical address
	let mut memory_top = 0;
	for e in avail_memory[..avail_memory_count].iter() {
		// SAFETY: all elements up to avail_memory_count have been written.
		let e = unsafe { e.assume_init() };
		if e.size > 0 { // shouldn't happen but let's be sure
			memory_top = memory_top.max(e.base + e.size - 1);
		}
	}

	// Remove regions occupied by the kernel
	let list = [(bt_bottom as u64, bt_top as u64), (kernel.start.into(), kernel.end.into())];
	let driver_list = drivers.iter().map(|d| (d.address.into(), (d.address + d.size).into()));
	for (bottom, top) in list.iter().copied().chain(driver_list) {
		for i in (0..avail_memory_count).rev() {
			// SAFETY: all elements up to avail_memory_count have been written.
			let e = unsafe { avail_memory[i].assume_init() };
			let (base, end) = (e.base, e.base + e.size);
			if bottom <= e.base && end <= top {
				// Discard the entire entry
				avail_memory_count -= 1;
				for i in i..avail_memory_count {
					unsafe {
						avail_memory[i].write(avail_memory[i].assume_init());
					}
				}
			} else if bottom < end && end <= top {
				// Cut off the top half
				avail_memory[i].write(info::MemoryRegion {
					base,
					size: bottom - base,
				});
			} else if bottom <= base && base < top {
				// Cut off the bottom half
				avail_memory[i].write(info::MemoryRegion {
					base: top,
					size: end - top,
				});
			} else if base <= bottom && top <= end {
				// Split the entry in half
				avail_memory[i].write(info::MemoryRegion {
					base,
					size: bottom - base,
				});
				avail_memory[avail_memory_count].write(info::MemoryRegion {
					base: top,
					size: end - top,
				});
				avail_memory_count += 1;
			}
		}
	}

	let avail_memory = unsafe {
		// SAFETY: all elements up to avail_memory_count have been written.
		MaybeUninit::slice_assume_init_mut(&mut avail_memory[..avail_memory_count])
	};

	// Set up page table
	let mut page_alloc_region = 0;
	let mut page_alloc = || {
		while avail_memory[page_alloc_region].size < 4096 || avail_memory[page_alloc_region].base == 0 {
			page_alloc_region += 1;
		}
		let page = avail_memory[page_alloc_region].base as *mut paging::Page;
		avail_memory[page_alloc_region].base += 4096;
		avail_memory[page_alloc_region].size -= 4096;
		unsafe { *page = paging::Page::zeroed() };
		page
	};

	let pml4 = page_alloc().cast::<paging::PML4>();
	let pml4 = unsafe {
		pml4.write(paging::PML4::new());
		&mut *pml4
	};

	unsafe {
		pml4.identity_map(&mut page_alloc, memory_top, &cpuid);
	}

	let kernel = unsafe {
		slice::from_raw_parts(
			kernel.start as *const u8,
			(kernel.end - kernel.start).try_into().unwrap(),
		)
	};

	let entry = elf64::load_elf(kernel, page_alloc, pml4).unwrap_or_else(|e| {
		print_err(b"Failed to load ELF: ");
		print_err(<&'static str as From<_>>::from(e).as_bytes());
		halt();
	});

	let info = unsafe {
		GDT_PTR.activate();
		INFO.set_memory_regions(avail_memory);
		INFO.set_drivers(drivers);
		&INFO
	};

	Return {
		entry,
		pml4,
		info,
	}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	/*
	#[cfg(not(debug_assertions))]
	unsafe {
		asm!("jmp	panic_function_is_present_");
	}
	*/
	print_err(b"Panic! ");
	if let Some(loc) = _info.location() {
		print_err(b"[");
		print_err(loc.file().as_bytes());
		print_err(b"]:");
		print_err_num(loc.line().into(), 10);
	}
	halt();
}

fn print_err(s: &[u8]) {
	debug_assert!(unsafe { VGA.is_some() });
	unsafe {
		VGA.as_mut().unwrap_unchecked().write_str(s, 0xc, 0)
	}
}

fn print_err_num(n: i128, base: u8) {
	debug_assert!(unsafe { VGA.is_some() });
	let _ = unsafe { VGA.as_mut().unwrap_unchecked().write_num(n, base, 0xc, 0) };
}

fn halt() -> ! {
	loop {
		unsafe {
			asm!("hlt");
		}
	}
}
