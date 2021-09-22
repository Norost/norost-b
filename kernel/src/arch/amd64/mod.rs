pub mod asm;
mod gdt;
mod multiboot;
mod tss;
pub mod r#virtual;

use core::mem::MaybeUninit;

static mut TSS: tss::TSS = tss::TSS::new();
static mut GDT: MaybeUninit<gdt::GDT> = MaybeUninit::uninit();
// TODO do we really need to keep this in memory forever?
static mut GDT_PTR: MaybeUninit<gdt::GDTPointer> = MaybeUninit::uninit();

pub unsafe fn init() {
	GDT.write(gdt::GDT::new(&TSS));
	GDT_PTR.write(gdt::GDTPointer::new(core::pin::Pin::new(
		GDT.assume_init_ref(),
	)));
	GDT_PTR.assume_init_mut().activate();
}

pub fn halt() -> ! {
	loop {
		unsafe { asm!("hlt") };
	}
}
