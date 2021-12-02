pub mod asm;
mod gdt;
#[macro_use]
mod idt;
pub mod msr;
mod multiboot;
mod syscall;
mod tss;
pub mod r#virtual;

pub use syscall::current_process;
pub use syscall::set_current_thread;

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
	IDT.set(61, idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(int noreturn handle_timer), 0));
	IDT.set(
		8,
		idt::IDTEntry::new(
			1 * 8,
			__idt_wrap_handler!(trap handle_double_fault),
			0,
		),
	);
	IDT.set(
		13,
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_general_protection_fault), 0),
	);
	IDT.set(
		14,
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_page_fault), 0),
	);
	IDT_PTR.write(idt::IDTPointer::new(&IDT));
	IDT_PTR.assume_init_ref().activate();

	syscall::init();
}

fn handle_timer(rip: *const ()) -> ! {
	debug!("Timer interrupt!");
	unsafe {
		debug!("  RIP:     {:p}", rip);
		crate::scheduler::next_thread()
	}
}

fn handle_double_fault(error: u32, rip: *const ()) {
	fatal!("Double fault!");
	unsafe {
		let addr: *const ();
		asm!("mov {}, cr2", out(reg) addr);
		fatal!("  error:   {:#x}", error);
		fatal!("  RIP:     {:p}", rip);
		fatal!("  address: {:p}", addr);
	}
	halt();
}

fn handle_general_protection_fault(error: u32, rip: *const ()) {
	fatal!("General protection fault!");
	unsafe {
		fatal!("  error:   {:#x}", error);
		fatal!("  RIP:     {:p}", rip);
	}
	halt();
}

fn handle_page_fault(error: u32, rip: *const ()) {
	unsafe { crate::log::force_unlock() };
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
