pub mod asm;
mod cpuid;
mod gdt;
#[macro_use]
pub mod idt;
pub mod msr;
mod multiboot;
mod syscall;
mod tss;
pub mod r#virtual;

use crate::{driver::apic, power, scheduler, time::Monotonic};
use core::arch::asm;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};
pub use idt::{Handler, IDTEntry};
pub use syscall::{
	current_process, current_thread, current_thread_weak, set_current_thread, ThreadData,
};

/// The IRQ used by the timer.
pub const TIMER_IRQ: u8 = 32;

static mut TSS: tss::TSS = tss::TSS::new();
static mut TSS_STACK: [usize; 512] = [0; 512];

static mut GDT: MaybeUninit<gdt::GDT> = MaybeUninit::uninit();
// TODO do we really need to keep this in memory forever?
static mut GDT_PTR: MaybeUninit<gdt::GDTPointer> = MaybeUninit::uninit();

static mut IDT: idt::IDT<256> = idt::IDT::new();
static mut IDT_PTR: MaybeUninit<idt::IDTPointer> = MaybeUninit::uninit();

// Start from 33, where IRQs 0..31 are used for exceptions and 32 is reserved for the timer.
static IRQ_ALLOCATOR: AtomicU8 = AtomicU8::new(33);

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
		TIMER_IRQ.into(),
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(int noreturn handle_timer), 0),
	);
	IDT.set(
		8,
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_double_fault), 0),
	);
	IDT.set(
		13,
		idt::IDTEntry::new(
			1 * 8,
			__idt_wrap_handler!(trap handle_general_protection_fault),
			0,
		),
	);
	IDT.set(
		14,
		idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_page_fault), 0),
	);
	IDT.set(16, idt::IDTEntry::new(1 * 8, idt::NOOP, 0));

	IDT_PTR.write(idt::IDTPointer::new(&IDT));
	IDT_PTR.assume_init_ref().activate();

	syscall::init();

	cpuid::enable_fsgsbase();
}

extern "C" fn handle_timer(rip: *const ()) -> ! {
	debug!("Timer interrupt!");
	debug!("  RIP:     {:p}", rip);
	apic::local_apic::get().eoi.set(0);
	unsafe { syscall::save_current_thread_state() };
	loop {
		if let Err(t) = unsafe { scheduler::next_thread() } {
			if let Some(d) = Monotonic::now().duration_until(t) {
				apic::set_timer_oneshot(d, Some(16));
				enable_interrupts();
				power::halt();
			}
		}
	}
}

extern "C" fn handle_double_fault(error: u32, rip: *const ()) {
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

extern "C" fn handle_general_protection_fault(error: u32, rip: *const ()) {
	fatal!("General protection fault!");
	fatal!("  error:   {:#x}", error);
	fatal!("  RIP:     {:p}", rip);
	halt();
}

extern "C" fn handle_page_fault(error: u32, rip: *const ()) {
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

pub fn halt() {
	unsafe { asm!("hlt") };
}

pub unsafe fn idt_set(irq: usize, entry: IDTEntry) {
	IDT.set(irq, entry);
}

pub fn yield_current_thread() {
	unsafe { asm!("int {}", const TIMER_IRQ) } // Fake timer interrupt
}

/// Allocate an IRQ ID.
///
/// This will fail if all IRQs from `0x00` to `0xFE` are allocated.
pub fn allocate_irq() -> Result<u8, IrqsExhausted> {
	IRQ_ALLOCATOR
		.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
			(n <= 0xfe).then(|| n + 1)
		})
		.map_err(|_| IrqsExhausted)
}

#[derive(Debug)]
pub struct IrqsExhausted;

#[inline(always)]
pub fn enable_interrupts() {
	unsafe {
		asm!("sti", options(nostack, nomem, preserves_flags));
	}
}

#[inline(always)]
pub fn disable_interrupts() {
	unsafe {
		asm!("cli", options(nostack, nomem, preserves_flags));
	}
}
