#![no_main]
#![no_std]
#![feature(asm)]

mod elf64;
mod gdt;
mod multiboot2;
mod paging;
mod vga;

use core::convert::TryInto;
use core::panic::PanicInfo;
use core::slice;

#[export_name = "gdt64"]
static GDT64: gdt::GDT64 = gdt::GDT64::new();

#[export_name = "main"]
fn main(magic: u32, arg: *const u8) -> ! {
	let mut vga = vga::Text::new();

	if magic != 0x36d76289 {
		vga.write_str(b"Bad multiboot2 magic: ", 0xc, 0);
		vga.write_num(magic.into(), 16, 0xc, 0);
		halt();
	}

	let mut avail_memory = None;
	let mut kernel = None;

	use multiboot2::bootinfo as bi;
	for e in unsafe { bi::BootInfo::new(arg) } {
		match e {
			bi::Info::Unknown(n) => {
				vga.write_str(b"Unknown info: ", 0xb, 0);
				vga.write_num(n.into(), 16, 0xb, 0);
				vga.write_str(b"\n", 0xb, 0);
			}
			bi::Info::Module(m) => {
				vga.write_str(b"Module: \"", 0xb, 0);
				vga.write_str(m.string, 0xb, 0);
				vga.write_str(b"\" ", 0xb, 0);
				vga.write_num(m.start.into(), 16, 0xb, 0);
				vga.write_str(b" - ", 0xb, 0);
				vga.write_num(m.end.into(), 16, 0xb, 0);
				vga.write_str(b"\n", 0xb, 0);
				if m.string == b"kernel" {
					kernel = Some(m);
				}
			}
			bi::Info::MemoryMap(m) => {
				vga.write_str(b"Memory map:\n", 0xb, 0);
				for e in m.entries.iter() {
					vga.write_str(b"  ", 0xb, 0);
					vga.write_num(e.base_address.into(), 16, 0xb, 0);
					vga.write_str(b" + ", 0xb, 0);
					vga.write_num(e.length.into(), 16, 0xb, 0);
					vga.write_str(b" (", 0xb, 0);
					vga.write_num(e.typ.into(), 16, 0xb, 0);
					vga.write_str(b")\n", 0xb, 0);
				}
				let e = m.entries.iter().find(|e| e.is_available()).unwrap();
				avail_memory = Some(e);
			}
		}
	}

	vga.write_str(b"Setting up PML4\n", 0xa, 0);

	let avail_memory = avail_memory.unwrap();
	let kernel = kernel.unwrap();

	let mut mem_start = avail_memory.base_address;
	let offt = (0x1000 - mem_start) & 0xfff;
	mem_start = mem_start + offt;
	let mut mem_count = (avail_memory.length - offt) / 0x1000;

	if mem_start == 0 {
		// TODO figure out if writing to null pointers is actually UB.
		// For now, just avoid it.
		mem_start += 0x1000;
		mem_count -= 1;
	}

	let mut page_alloc = move || {
		assert!(mem_count > 0);
		mem_start += 4096;
		mem_count -= 1;
		let page = mem_start as *mut paging::Page;
		unsafe { *page = paging::Page::zeroed() };
		page
	};

	let pml4 = page_alloc().cast::<paging::PML4>();
	let pml4 = unsafe {
		pml4.write(paging::PML4::new());
		&mut *pml4
	};

	unsafe {
		pml4.identity_map(&mut page_alloc);
	}

	let kernel = unsafe {
		slice::from_raw_parts(
			kernel.start as *const u8,
			(kernel.end - kernel.start).try_into().unwrap(),
		)
	};
	let entry = elf64::load_elf(kernel, page_alloc, pml4);

	vga.write_str(b"FUCK\n", 0xa, 0);

	unsafe {
		pml4.activate();
	}
	vga.write_str(b"GDT\n", 0xa, 0);

	let gdt = gdt::GDT64::new();
	let gdt = core::pin::Pin::new(&gdt);
	let gdt = gdt::GDT64Pointer::new(gdt);
	unsafe {
		gdt.activate();
	}

	vga.write_str(b"Done\n", 0xa, 0);

	unsafe {
		let (el, eh) = (entry as u32, (entry >> 32) as u32);
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
		", in("eax") el, in("ebx") eh, in("edi") arg, options(noreturn, att_syntax));
	}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	let mut vga = vga::Text::new();
	vga.write_str(b"Panic! ", 0xc, 0);
	if let Some(loc) = _info.location() {
		vga.write_str(b"[", 0xc, 0);
		vga.write_str(loc.file().as_bytes(), 0xc, 0);
		vga.write_str(b"]:", 0xc, 0);
		vga.write_num(loc.line().into(), 10, 0xc, 0);
	}
	halt();
}

fn halt() -> ! {
	loop {
		unsafe {
			asm!("hlt");
		}
	}
}
