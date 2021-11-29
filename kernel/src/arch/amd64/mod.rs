pub mod asm;
mod gdt;
#[macro_use]
mod idt;
mod msr;
mod multiboot;
mod syscall;
mod tss;
pub mod r#virtual;

pub use syscall::current_process;

use core::mem::MaybeUninit;

static mut TSS: tss::TSS = tss::TSS::new();
static mut TSS_STACK: [usize; 512] = [0; 512];

static mut GDT: MaybeUninit<gdt::GDT> = MaybeUninit::uninit();
// TODO do we really need to keep this in memory forever?
static mut GDT_PTR: MaybeUninit<gdt::GDTPointer> = MaybeUninit::uninit();

static mut IDT: idt::IDT<256> = idt::IDT::new();
static mut IDT_PTR: MaybeUninit<idt::IDTPointer> = MaybeUninit::uninit();

pub unsafe fn init() {
	// Setup TSS
	TSS.set_rsp(0, TSS_STACK.as_ptr());

	// Setup GDT
	GDT.write(gdt::GDT::new(&TSS));
	GDT_PTR.write(gdt::GDTPointer::new(core::pin::Pin::new(
		GDT.assume_init_ref(),
	)));
	GDT_PTR.assume_init_mut().activate();

	// Setup IDT
	IDT.set(
		8,
		idt::IDTEntry::new(
			1 * 8,
			|| {
				fatal!("Double fault!");
				halt();
			},
			true,
			0,
		),
	);
	IDT.set(
		14,
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(handle_page_fault), true, 0),
	);
	IDT_PTR.write(idt::IDTPointer::new(&IDT));
	IDT_PTR.assume_init_ref().activate();

	syscall::init();
}

fn handle_page_fault(error: u32, rip: *const ()) {
	fatal!("Page fault!");
	unsafe {
		let addr: *const ();
		asm!("mov {}, cr2", out(reg) addr);
		fatal!("  error:   {:#x}", error);
		fatal!("  RIP:     {:p}", rip);
		fatal!("  address: {:p}", addr);
	}
	halt();
}

pub fn halt() -> ! {
	loop {
		unsafe { asm!("hlt") };
	}
}
