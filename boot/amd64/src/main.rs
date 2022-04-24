#![no_main]
#![no_std]
#![feature(alloc_layout_extra)]
#![feature(asm_const)]
#![feature(byte_slice_trim_ascii)]
#![feature(inline_const)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_slice)]

mod alloc;
mod cpuid;
mod elf64;
mod gdt;
mod info;
mod msr;
mod mtrr;
mod multiboot2;
mod paging;
mod uart;
mod vga;

use alloc::alloc;
use core::alloc::Layout;
use core::arch::asm;
use core::fmt::{self, Write};
use core::mem::{self, MaybeUninit};
use core::panic::PanicInfo;
use core::slice;

#[link_section = ".init.gdt"]
static GDT: gdt::GDT = gdt::GDT::new();
static GDT_PTR: gdt::GDTPointer = gdt::GDTPointer::new(&GDT);

#[repr(C)]
struct Return {
	entry: u64,
	pml4: &'static paging::PML4,
	buffer: *mut u8,
}

static mut VGA: Option<vga::Text> = None;

fn alloc_str(arg: &[u8]) -> u16 {
	let layout = Layout::from_size_align(1 + arg.len(), 1).unwrap();
	let s = unsafe { slice::from_raw_parts_mut(alloc(layout), layout.size()) };
	s[1..arg.len() + 1].copy_from_slice(arg);
	s[0] = s.len().try_into().unwrap();
	alloc::offset(s.as_ptr())
}

fn alloc_slice<T>(count: usize) -> (u16, &'static mut [T]) {
	let layout = Layout::new::<T>().repeat(count).unwrap();
	let s = unsafe { slice::from_raw_parts_mut::<T>(alloc(layout.0).cast(), count) };
	(alloc::offset(s.as_ptr().cast()), s)
}

#[export_name = "main"]
extern "fastcall" fn main(magic: u32, arg: *const u8) -> Return {
	unsafe {
		VGA = Some(vga::Text::new());
	}

	let cpuid = cpuid::Features::new().expect("No CPUID support");

	assert_eq!(magic, 0x36d76289, "Bad multiboot2 magic");

	use multiboot2::bootinfo as bi;

	let mut kernel = None;
	let mut init = None;

	let mut rsdp = None;

	let info = unsafe { &mut *alloc(Layout::new::<info::Info>()).cast::<info::Info>() };

	// Parsing BootInfo is done in multiple passes so that the buffer is packed tightly &
	// no reallocations are necessary.
	let boot_info = || unsafe { bi::BootInfo::new(arg) };

	// Find kernel, init & RSDP but just count amount of drivers & ignore the rest
	for e in boot_info() {
		match e {
			bi::Info::Unknown(_) => {}
			bi::Info::Module(m) => match m.string {
				b"kernel" => {
					assert!(kernel.is_none(), "kernel has already been specified");
					kernel = Some(m);
				}
				b"init" => {
					assert!(init.is_none(), "init has already been specified");
					init = Some(m);
				}
				s if s.starts_with(b"driver ") || s.starts_with(b"driver\t") => {
					info.drivers_len += 1
				}
				m => panic!("unknown module type: {:?}", core::str::from_utf8(m)),
			},
			bi::Info::MemoryMap(_) => {}
			bi::Info::AcpiRsdp(r) => rsdp = Some(r),
		}
	}

	// Only parse drivers so we can exclude them from the final memory regions & we need
	// them for init.
	let (offset, drivers) = alloc_slice(info.drivers_len.into());
	info.drivers_offset = offset;
	let mut i = 0;
	for e in boot_info() {
		if let bi::Info::Module(m) = e {
			if m.string.starts_with(b"driver ") || m.string.starts_with(b"driver\t") {
				let name = m.string[b"driver ".len()..].trim_ascii();
				assert!(name.len() < 16, "name may not be longer than 15 characters");
				drivers[i] = info::Driver {
					address: m.start as u32,
					size: (m.end - m.start).try_into().unwrap(),
					name_offset: alloc_str(name),
					_padding: 0,
				};
				i += 1;
			} else if m.string == b"driver" {
				panic!("driver must have a name");
			}
		}
	}

	// Parse init programs
	let init = init.expect("no init specified");
	let text = unsafe {
		slice::from_raw_parts(
			init.start as *const u8,
			(init.end - init.start).try_into().unwrap(),
		)
	};
	let lines_iter = || {
		text.split(|c| *c == b'\n')
			.flat_map(|l| l.split(|c| *c == b'#').next())
			.map(|l| l.trim_ascii())
			.filter(|l| !l.is_empty())
	};
	let (offset, init) = alloc_slice::<info::InitProgram>(lines_iter().count());
	info.init_offset = offset;
	info.init_len = init.len().try_into().unwrap();
	for (i, (line, init)) in lines_iter().zip(init).enumerate() {
		// Split into words
		let mut words = line
			.split(|c| b"\t ".contains(c))
			.filter(|l| !l.is_empty())
			.peekable();

		// Get program name & find the corresponding index.
		let program = words.next().expect("no program name specified");
		init.driver = u16::MAX;

		// Parse program arguments
		// We rely on the fact that alloc() is a bump allocator.
		for arg in words {
			let s = alloc_str(arg);
			if init.args_offset == 0 {
				init.args_offset = s;
			}
		}
	}

	assert_ne!(info.init_len, 0, "no init programs specified");
	let kernel = kernel.expect("No kernel module");
	info.rsdp.write(*rsdp.unwrap());

	// Determine free memory regions
	let iter_regions = |callback: &mut dyn FnMut(info::MemoryRegion)| {
		let apply = &mut |base: u64, size: u64| {
			let mut callback = |start, end| {
				callback(info::MemoryRegion {
					base: start,
					size: end - start,
				})
			};
			let kernel_list = [(kernel.start.into(), kernel.end.into())];
			let driver_list = drivers
				.iter()
				.map(|d| (d.address.into(), (d.address + d.size).into()));
			for (bottom, top) in kernel_list.iter().copied().chain(driver_list) {
				let (start, end) = (base, base + size);
				if bottom <= start && end <= top {
					// Discard the entire entry
				} else if bottom < end && end <= top {
					// Cut off the top half
					callback(start, bottom);
				} else if bottom <= start && start < top {
					// Cut off the bottom half
					callback(top, end);
				} else if start <= bottom && top <= end {
					// Split the entry in half
					callback(start, bottom);
					callback(top, end);
				}
			}
		};

		for e in boot_info() {
			if let bi::Info::MemoryMap(m) = e {
				for e in m.entries.iter().filter(|e| e.is_available()) {
					assert_eq!(e.base_address & 0xfff, 0, "misaligned base address");
					/* It *can* happen... I have no idea why though.
					assert_eq!(
						e.length & 0xfff,
						0,
						"length is not a multiple of the page size"
					);
					*/
					if e.base_address == 0 {
						// Split of the first page so we can avoid writing to null (which is ub)
						apply(0, 4096);
						if let Some(l) = e.length.checked_sub(4096) {
							apply(4096, l);
						}
					} else {
						apply(e.base_address, e.length)
					}
				}
			}
		}
	};

	// Determine (guess) the maximum valid physical address & count the amount of regions
	let mut memory_top = 0;
	let mut memory_regions_count = 0;
	iter_regions(&mut |region| {
		assert!(region.size > 0, "empty region makes no sense");
		memory_top = memory_top.max(region.base + region.size - 1);
		memory_regions_count += 1;
	});

	// Collect all memory regions excluding area occupied by the kernel & drivers
	let (offset, mut memory_regions) = alloc_slice::<info::MemoryRegion>(memory_regions_count);
	info.memory_regions_offset = offset;
	info.memory_regions_len = memory_regions.len().try_into().unwrap();
	let mut i = 0;
	iter_regions(&mut |region| {
		memory_regions[i] = region;
		i += 1;
	});

	// Set up page table
	let mut page_alloc_region = 0;
	let mut page_alloc = || {
		while memory_regions[page_alloc_region].size < 4096
			// Exclude null address since dereferencing null is UB.
			|| memory_regions[page_alloc_region].base == 0
		{
			page_alloc_region += 1;
		}
		let page = memory_regions[page_alloc_region].base as *mut paging::Page;
		memory_regions[page_alloc_region].base += 4096;
		memory_regions[page_alloc_region].size -= 4096;
		unsafe { *page = paging::Page::zeroed() };
		page
	};

	let pml4 = page_alloc().cast::<paging::PML4>();
	let pml4 = unsafe {
		pml4.write(paging::PML4::new());
		&mut *pml4
	};
	todo!();

	unsafe {
		pml4.identity_map(&mut page_alloc, memory_top, &cpuid);
	}

	// TODO we should remove empty memory regions.

	let kernel = unsafe {
		slice::from_raw_parts(
			kernel.start as *const u8,
			(kernel.end - kernel.start).try_into().unwrap(),
		)
	};

	let entry = elf64::load_elf(kernel, page_alloc, pml4).expect("Failed to load ELF: {}");

	unsafe {
		GDT_PTR.activate();
	}

	Return {
		entry,
		pml4,
		buffer: alloc::buffer_ptr(),
	}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	let _ = write!(Stderr, "{}", info);
	halt();
}

struct Stderr;

impl Write for Stderr {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let s = s.as_bytes();
		debug_assert!(unsafe { VGA.is_some() });
		unsafe { VGA.as_mut().unwrap_unchecked().write_str(s, 0xc, 0) }
		s.iter().copied().for_each(uart::Uart::send);
		Ok(())
	}
}

fn halt() -> ! {
	loop {
		unsafe {
			asm!("hlt");
		}
	}
}
