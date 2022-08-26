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

macro_rules! log {
	($fmt:literal) => {{
		let _ = $crate::Stdout.write_str(concat!($fmt, "\n"));
	}};
	($fmt:literal, $($arg:tt)+) => {{
		let _ = writeln!($crate::Stdout, $fmt, $($arg)+);
	}};
}

macro_rules! err {
	($fmt:literal) => {{
		let _ = $crate::Stderr.write_str(concat!($fmt, "\n"));
	}};
	($fmt:literal, $($arg:tt)+) => {{
		let _ = writeln!($crate::Stderr, $fmt, $($arg)+);
	}};
}

use alloc::alloc;
use core::alloc::Layout;
use core::arch::asm;
use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::slice;
use core::str;

extern "C" {
	static boot_bottom: usize;
	static boot_top: usize;
}

#[link_section = ".init.gdt"]
#[export_name = "_gdt"]
static GDT: gdt::GDT = gdt::GDT::new();
#[export_name = "_gdt_ptr32"]
static GDT_PTR: gdt::GDTPointer = gdt::GDTPointer::new(&GDT);

#[repr(C)]
struct Return {
	entry: u64,
	pml4: &'static paging::PML4,
	buffer: *mut u8,
}

static mut VGA: Option<vga::Text> = None;

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
	let mut initfs = None;
	let mut rsdp = None;

	let (boot_start, boot_end) = unsafe {
		(
			&boot_bottom as *const _ as u64,
			&boot_top as *const _ as u64,
		)
	};
	log!("Boot: {:#x} - {:#x}", boot_start, boot_end);

	let info = unsafe { &mut *alloc(Layout::new::<info::Info>()).cast::<info::Info>() };

	// Parsing BootInfo is done in multiple passes so that the buffer is packed tightly &
	// no reallocations are necessary.
	let boot_info = || unsafe { bi::BootInfo::new(arg) };

	let mut memory_top = None;

	// Find initfs, kernel & RSDP but just count amount of drivers & ignore the rest
	for e in boot_info() {
		match e {
			bi::Info::Unknown(ty) => {
				err!("multiboot2: unknown type {}", ty)
			}
			bi::Info::Module(m) => match m.string {
				b"initfs" => {
					assert!(initfs.is_none(), "initfs has already been specified");
					initfs = Some(m)
				}
				b"kernel" => {
					assert!(kernel.is_none(), "kernel has already been specified");
					kernel = Some(m);
				}
				m => panic!("unknown module type: {:?}", core::str::from_utf8(m)),
			},
			bi::Info::MemoryMap(m) => {
				log!("multiboot2: memory map");
				assert!(
					memory_top.is_none(),
					"memory map has already been specified"
				);
				let mut max = 0;
				for e in m.entries {
					let _ = log!("  {:#10x} {:#10x} {:05}", e.base_address, e.length, e.typ);
					max = max.max(e.base_address + e.length);
				}
				memory_top = Some(max - 1);
			}
			bi::Info::AcpiRsdp(r) => rsdp = Some(r),
			bi::Info::FramebufferInfo(fb) => {
				let f = |n: u32| n.checked_sub(1).and_then(|n| n.try_into().ok());
				info.framebuffer = info::Framebuffer {
					base: fb.addr,
					pitch: f(fb.pitch).expect("pitch out of range"),
					width: f(fb.width).expect("width out of range"),
					height: f(fb.height).expect("height out of range"),
					bpp: fb.bpp,
					..info.framebuffer
				};
				match fb.color_info {
					bi::FramebufferColorInfo::IndexedColor(_) => {
						err!("todo: indexed color")
					}
					bi::FramebufferColorInfo::DirectRgbColor(ci) => {
						info.framebuffer = info::Framebuffer {
							r_pos: ci.r_pos,
							g_pos: ci.g_pos,
							b_pos: ci.b_pos,
							r_mask: ci.r_mask,
							g_mask: ci.g_mask,
							b_mask: ci.b_mask,
							..info.framebuffer
						};
					}
					bi::FramebufferColorInfo::EgaText => {
						err!("todo: EGA text")
					}
					bi::FramebufferColorInfo::Unknown(ty) => {
						err!("unknown framebuffer type {}", ty);
					}
				}
			}
		}
	}

	let kernel = kernel.expect("No kernel");
	let initfs = initfs.expect("No initfs");
	let memory_top = memory_top.expect("no memory map");
	log!("kernel: {:#x} - {:#x}", kernel.start, kernel.end);
	log!("initfs: {:#x} - {:#x}", initfs.start, initfs.end);
	info.rsdp.write(*rsdp.expect("no RSDP found"));

	// Determine free memory regions
	let iter_regions = |callback: &mut dyn FnMut(info::MemoryRegion)| {
		fn apply(
			base: u64,
			size: u64,
			callback: &mut dyn FnMut(info::MemoryRegion),
			mut reserved: impl Iterator<Item = (u64, u64)> + Clone,
		) {
			if let Some((bottom, top)) = reserved.next() {
				let mut apply = |start, end| {
					// Align the addresses to a page boundary.
					let start = (start + 0xfff) & !0xfff;
					let end = end & !0xfff;
					// Discard the region if it's zero-sized.
					if start != end {
						apply(start, end - start, callback, reserved.clone())
					}
				};
				let (start, end) = (base, base + size);
				if bottom <= start && end <= top {
					// b----s-_-_e----t
					// Discard the entire region
				} else if bottom < end && end <= top {
					// s____b-_-_e----t
					// Cut off the top half
					apply(start, bottom);
				} else if bottom <= start && start < top {
					// b----s-_-_t____e
					// Cut off the bottom half
					apply(top, end);
				} else if start <= bottom && top <= end {
					// s____b-_-_t____e
					// Split the entry in half
					apply(start, bottom);
					apply(top, end);
				} else {
					// Don't split
					apply(start, end)
				}
			} else {
				// There is nothing left to split
				callback(info::MemoryRegion { base, size })
			}
		}
		let apply = &mut |base, size| {
			let list = [
				(boot_start, boot_end),
				(kernel.start.into(), kernel.end.into()),
				(initfs.start.into(), initfs.end.into()),
			];
			apply(base, size, callback, list.into_iter())
		};

		for e in boot_info() {
			if let bi::Info::MemoryMap(m) = e {
				for e in m.entries.iter().filter(|e| e.is_available()) {
					assert_eq!(e.base_address & 0xfff, 0, "misaligned base address");
					/* It *can* happen in some cases. No unaligned base addresses so far though.
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

	// Count the amount of regions
	let mut memory_regions_count = 0;
	iter_regions(&mut |region| {
		assert!(region.size > 0, "empty region makes no sense");
		memory_regions_count += 1;
	});

	// Collect all memory regions excluding area occupied by the kernel & drivers
	let (offset, memory_regions) = alloc_slice::<info::MemoryRegion>(memory_regions_count);
	info.memory_regions_offset = offset;
	info.memory_regions_len = memory_regions.len().try_into().unwrap();
	info.initfs_ptr = initfs.start;
	info.initfs_len = initfs.end - initfs.start;
	let mut i = 0;
	iter_regions(&mut |region| {
		log!(
			"Memory region: {:#x} - {:#x}",
			region.base,
			region.base + region.size
		);
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

	// Set IA32_PAT so we can use all caching types in a sensible way
	unsafe {
		// 0: WB
		// 1: WC
		// Repeat for 4-7 because FIXME why oh why do you use both bit 12 and 7 Intel.
		// Especially 7 is already used to indicate whether a page is a 2M page, which
		// I incidentally am using for 4K pages. Bloody shit
		msr::wrmsr(msr::IA32_PAT, 0x0000_0106_0000_0106);
	}

	Return {
		entry,
		pml4,
		buffer: alloc::buffer_ptr(),
	}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
	err!("{}", info);
	halt();
}

struct Stderr;

struct Stdout;

impl Write for Stderr {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let s = s.as_bytes();
		debug_assert!(unsafe { VGA.is_some() });
		unsafe { VGA.as_mut().unwrap_unchecked().write_str(s, 0xc, 0) }
		s.iter().copied().for_each(uart::Uart::send);
		Ok(())
	}
}

impl Write for Stdout {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		let s = s.as_bytes();
		debug_assert!(unsafe { VGA.is_some() });
		unsafe { VGA.as_mut().unwrap_unchecked().write_str(s, 7, 0) }
		s.iter().copied().for_each(uart::Uart::send);
		Ok(())
	}
}

fn halt() -> ! {
	unsafe {
		// Interrupts are not enabled.
		asm!("hlt", options(nostack, nomem, noreturn));
	}
}
