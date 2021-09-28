pub mod asm;
mod gdt;
mod idt;
mod multiboot;
mod tss;
pub mod r#virtual;

use core::mem::MaybeUninit;

static mut TSS: tss::TSS = tss::TSS::new();
static mut GDT: MaybeUninit<gdt::GDT> = MaybeUninit::uninit();
// TODO do we really need to keep this in memory forever?
static mut GDT_PTR: MaybeUninit<gdt::GDTPointer> = MaybeUninit::uninit();

static mut IDT: idt::IDT<256> = idt::IDT::new();
static mut IDT_PTR: MaybeUninit<idt::IDTPointer> = MaybeUninit::uninit();

pub unsafe fn init() {
	GDT.write(gdt::GDT::new(&TSS));
	GDT_PTR.write(gdt::GDTPointer::new(core::pin::Pin::new(
		GDT.assume_init_ref(),
	)));
	GDT_PTR.assume_init_mut().activate();

	IDT.set(8, idt::IDTEntry::new(1 * 8, || {
		fatal!("Double fault!");
		halt();
	}, true, 0));
	IDT_PTR.write(idt::IDTPointer::new(&IDT));
	IDT_PTR.assume_init_ref().activate();
}

pub fn halt() -> ! {
	loop {
		unsafe { asm!("hlt") };
	}
}
