#![no_main]
#![no_std]
#![feature(asm)]
#![feature(maybe_uninit_uninit_array, maybe_uninit_slice)]

mod elf64;
mod gdt;
mod info;
mod multiboot2;
mod paging;
mod vga;

use core::convert::TryInto;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::slice;

static GDT: gdt::GDT = gdt::GDT::new();
static GDT_PTR: gdt::GDTPointer = gdt::GDTPointer::new(&GDT);

extern "C" {
	static stack_top: usize;
	static stack_bottom: usize;
}

#[export_name = "main"]
fn main(magic: u32, arg: *const u8) -> ! {
	let mut vga = vga::Text::new();

	if magic != 0x36d76289 {
		vga.write_str(b"Bad multiboot2 magic: ", 0xc, 0);
		vga.write_num(magic.into(), 16, 0xc, 0).unwrap_or_else(|_| unreachable!());
		halt();
	}

	let mut avail_memory = MaybeUninit::uninit_array::<8>();
	let mut avail_memory_count = 0;
	let mut kernel = None;

	let (stk_top, stk_bottom) = unsafe {
		(&stack_top as *const _ as usize, &stack_bottom as *const _ as usize)
	};

	use multiboot2::bootinfo as bi;
	for e in unsafe { bi::BootInfo::new(arg) } {
		match e {
			bi::Info::Unknown(_) => (),
			bi::Info::Module(m) => {
				if m.string == b"kernel" {
					kernel = Some(m);
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
		print_err(&mut vga, b"No kernel module");
		halt();
	});

	// Determine (guess) the maximum valid physical address
	let mut memory_top = 0;
	for e in avail_memory[..avail_memory_count].iter() {
		// SAFETY: all elements up to avail_memory_count have been written.
		let e = unsafe { e.assume_init() };
		if e.size > 0 { // shouldn't happen but let's be sure
			memory_top = memory_top.max(e.base + e.size - 1);
		}
	}

	// Remove regions occupied by the kernel (FIXME replace with actual kernel fuckwit)
	for i in (0..avail_memory_count).rev() {
		let list = [(stk_bottom as u64, stk_top as u64), (kernel.start.into(), kernel.end.into())];
		for (bottom, top) in list.iter().copied() {
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
		pml4.identity_map(&mut page_alloc, memory_top);
	}

	let kernel = unsafe {
		slice::from_raw_parts(
			kernel.start as *const u8,
			(kernel.end - kernel.start).try_into().unwrap(),
		)
	};

	let entry = elf64::load_elf(kernel, page_alloc, pml4).unwrap_or_else(|e| {
		vga.write_str(b"Failed to load ELF: ", 0xc, 0);
		vga.write_str(<&'static str as From<_>>::from(e).as_bytes(), 0xc, 0);
		halt();
	});

	let info = info::Info::new(avail_memory, (stk_top, stk_bottom));

	unsafe {
		pml4.activate();
		let (el, eh) = (entry as u32, (entry >> 32) as u32);
		GDT_PTR.activate();
		asm!("
			# Switch to long mode
			ljmp	$0x8, $realm64
		.code64
		realm64:

			# Fix entry address
			mov		$32, %cl
			shlq	%cl, %rbx
			orq		%rax, %rbx

			# Setup data segment properly
			mov		$0x10, %ax
			mov		%ax, %ds
			mov		%ax, %es
			mov		%ax, %fs
			mov		%ax, %gs
			mov		%ax, %ss

			# Jump to kernel entry
			jmp		*%rbx
		", in("eax") el, in("ebx") eh, in("edi") &info, options(noreturn, att_syntax));
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
	let mut vga = vga::Text::new();
	vga.write_str(b"Panic! ", 0xc, 0);
	if let Some(loc) = _info.location() {
		vga.write_str(b"[", 0xc, 0);
		vga.write_str(loc.file().as_bytes(), 0xc, 0);
		vga.write_str(b"]:", 0xc, 0);
		vga.write_num(loc.line().into(), 10, 0xc, 0).unwrap_or_else(|_| unreachable!());
	}
	halt();
}

fn print_err(vga: &mut vga::Text, s: &[u8]) {
	vga.write_str(s, 0xc, 0);
}

fn halt() -> ! {
	loop {
		unsafe {
			asm!("hlt");
		}
	}
}
