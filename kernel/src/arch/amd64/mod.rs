pub mod asm;
mod gdt;
mod idt;
mod multiboot;
mod msr;
mod syscall;
mod tss;
pub mod r#virtual;

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
	IDT.set(8, idt::IDTEntry::new(1 * 8, || {
		fatal!("Double fault!");
		halt();
	}, true, 0));
	IDT_PTR.write(idt::IDTPointer::new(&IDT));
	IDT_PTR.assume_init_ref().activate();

	syscall::init();
}

pub fn halt() -> ! {
	loop {
		unsafe { asm!("hlt") };
	}
}
