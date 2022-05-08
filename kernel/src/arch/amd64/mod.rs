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

use crate::{driver::apic, scheduler};
use core::arch::asm;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};
pub use idt::{Handler, IDTEntry};
pub use syscall::{
	clear_current_thread, current_process, current_thread, current_thread_weak, set_current_thread,
	ThreadData,
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

static mut DOUBLE_FAULT_STACK: [usize; 512] = [0; 512];

pub unsafe fn init() {
	unsafe {
		// Setup TSS
		TSS.set_rsp(0, TSS_STACK.as_ptr().add(512));
		TSS.set_ist(1.try_into().unwrap(), DOUBLE_FAULT_STACK.as_ptr().add(512));

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
			6,
			idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_invalid_opcode), 0),
		);
		IDT.set(
			8,
			idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_double_fault), 1),
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

		r#virtual::init();
	}
}

extern "C" fn handle_timer(_rip: *const ()) -> ! {
	debug!("Timer interrupt!");
	debug!("  RIP:     {:p}", _rip);
	apic::local_apic::get().eoi.set(0);
	unsafe { syscall::save_current_thread_state() };
	// SAFETY: we just saved the thread's state.
	unsafe { scheduler::next_thread() }
}

extern "C" fn handle_invalid_opcode(error: u32, rip: *const ()) {
	fatal!("Invalid opcode!");
	unsafe {
		let addr: *const ();
		asm!("mov {}, cr2", out(reg) addr);
		fatal!("  error:   {:#x}", error);
		fatal!("  RIP:     {:p}", rip);
		fatal!("  address: {:p}", addr);
	}
	halt();
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
	unsafe {
		IDT.set(irq, entry);
	}
}

pub fn yield_current_thread() {
	unsafe { asm!("int {}", const TIMER_IRQ) } // Fake timer interrupt
}

/// Switch to this CPU's local stack and call the given function.
///
/// This macro is intended for cleaning up processes & threads.
pub macro run_on_local_cpu_stack_noreturn($f: path, $data: expr) {
	const _: extern "C" fn(*const ()) -> ! = $f;
	let data: *const () = $data;
	unsafe {
		asm!(
			"cli",
			"push rbp",
			"mov  rbp, rsp",
			"mov  rsp, {stack}",
			"jmp {f}",
			f = sym $f,
			stack = in(reg) $crate::arch::amd64::_cpu_stack(),
			in("rdi") data,
			options(nostack, noreturn),
		)
	}
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
		asm!("sti", options(nostack, preserves_flags));
	}
}

#[inline(always)]
pub fn disable_interrupts() {
	unsafe {
		asm!("cli", options(nostack, preserves_flags));
	}
}

pub fn _cpu_stack() -> *mut () {
	syscall::cpu_stack()
}
