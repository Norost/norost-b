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

/// The IRQ offset of the master PIC.
pub const PIC1_IRQ_OFFSET: u8 = 32;
/// The IRQ offset of the slae PIC.
pub const PIC2_IRQ_OFFSET: u8 = 40;
/// The IRQ used by the timer.
pub const TIMER_IRQ: u8 = 48;

static mut TSS: tss::TSS = tss::TSS::new();

static mut GDT: MaybeUninit<gdt::GDT> = MaybeUninit::uninit();
// TODO do we really need to keep this in memory forever?
static mut GDT_PTR: MaybeUninit<gdt::GDTPointer> = MaybeUninit::uninit();

static mut IDT: idt::IDT<256> = idt::IDT::new();
static mut IDT_PTR: MaybeUninit<idt::IDTPointer> = MaybeUninit::uninit();

// Start from 33, where IRQs 0..31 are used for exceptions and 32 is reserved for the timer.
static IRQ_ALLOCATOR: AtomicU8 = AtomicU8::new(33);

static mut DOUBLE_FAULT_STACK: [usize; 512] = [0; 512];

pub mod pic {
	//! https://wiki.osdev.org/PIC

	use super::asm::io::{inb, outb};

	/// IO base address for master PIC
	pub const PIC1: u16 = 0x20;
	/// IO base address for slave PIC
	pub const PIC2: u16 = 0xa0;
	pub const PIC1_COMMAND: u16 = PIC1;
	pub const PIC1_DATA: u16 = PIC1 + 1;
	pub const PIC2_COMMAND: u16 = PIC2;
	pub const PIC2_DATA: u16 = PIC2 + 1;

	/// End-of-interrupt command code
	#[allow(dead_code)]
	pub const EOI: u8 = 0x20;

	/// ICW4 (not) needed
	pub const ICW1_ICW4: u8 = 0x01;
	/// Single (cascade) mode
	#[allow(dead_code)]
	pub const ICW1_SINGLE: u8 = 0x02;
	/// Call address interval 4 (8)
	#[allow(dead_code)]
	pub const ICW1_INTERVAL4: u8 = 0x04;
	/// Level triggered (edge) mode
	#[allow(dead_code)]
	pub const ICW1_LEVEL: u8 = 0x08;
	/// Initialization - required!
	#[allow(dead_code)]
	pub const ICW1_INIT: u8 = 0x10;

	/// 8086/88 (MCS-80/85) mode
	pub const ICW4_8086: u8 = 0x01;
	/// Auto (normal) EOI
	#[allow(dead_code)]
	pub const ICW4_AUTO: u8 = 0x02;
	/// Buffered mode/slave
	#[allow(dead_code)]
	pub const ICW4_BUF_SLAVE: u8 = 0x08;
	/// Buffered mode/master
	#[allow(dead_code)]
	pub const ICW4_BUF_MASTER: u8 = 0x0c;
	/// Special fully nested (not)
	#[allow(dead_code)]
	pub const ICW4_SFNM: u8 = 0x10;

	/// Initialize the PIC. This will remap the interrupts and mask all of them.
	///
	/// They are all masked by default because we don't need them normally. Drivers that
	/// do need them should enable them manually (e.g. PIC driver).
	///
	/// # Safety
	///
	/// This function must be called exactly once.
	pub(super) unsafe fn init() {
		unsafe {
			// Setup PIC
			// ICW1 (allow ICW4 & set PIC to be initialized)
			outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
			outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
			// ICW2 (map IVT)
			outb(PIC1_DATA, super::PIC1_IRQ_OFFSET);
			outb(PIC2_DATA, super::PIC2_IRQ_OFFSET);
			// ICW3 (tell master (PIC1) about slave (PIC2) & vice versa)
			outb(PIC1_DATA, 4);
			outb(PIC2_DATA, 2);
			// ICW4 (set 80x86 mode)
			outb(PIC1_DATA, ICW4_8086);
			outb(PIC2_DATA, ICW4_8086);
			// Mask all interrupts
			outb(PIC1_DATA, 0xff);
			outb(PIC2_DATA, 0xff);
		}
	}

	#[allow(dead_code)]
	#[derive(Clone, Copy)]
	enum Ocw3 {
		ReadIrr = 0xa,
		ReadIsr = 0xb,
	}

	#[allow(dead_code)]
	fn irq_reg(ocw3: Ocw3) -> u16 {
		unsafe {
			outb(PIC1_COMMAND, ocw3 as u8);
			outb(PIC2_COMMAND, ocw3 as u8);
			u16::from(inb(PIC1_COMMAND)) | (u16::from(inb(PIC2_COMMAND)) << 8)
		}
	}
}

pub unsafe fn init() {
	unsafe {
		// Remap IBM-PC interrupts
		// Even if the PIC is disabled it may generate spurious interrupts apparently *sigh*
		pic::init();

		// Setup TSS
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
			idt::IDTEntry::new(
				1 * 8,
				__idt_wrap_handler!(int noreturn savethread handle_timer),
				0,
			),
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
		IDT.set(
			16,
			idt::IDTEntry::new(1 * 8, __idt_wrap_handler!(trap handle_x87_fpe), 0),
		);

		IDT_PTR.write(idt::IDTPointer::new(&IDT));
		IDT_PTR.assume_init_ref().activate();

		syscall::init();

		cpuid::enable_fsgsbase();

		r#virtual::init();
	}
}

extern "C" fn handle_timer() -> ! {
	debug!("Timer interrupt!");
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

extern "C" fn handle_x87_fpe(error: u32, rip: *const ()) {
	fatal!("x87 FPE!");
	fatal!("  error:   {:#x}", error);
	fatal!("  RIP:     {:p}", rip);
	halt();
}

pub fn halt() {
	unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) };
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
pub fn interrupts_enabled() -> bool {
	unsafe {
		let flags: usize;
		asm!("pushf; pop {}", out(reg) flags, options(preserves_flags));
		flags & (1 << 9) != 0
	}
}

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
