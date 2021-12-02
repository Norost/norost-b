pub mod local_apic;

use crate::arch::amd64::msr;
use crate::memory::Page;
use acpi::{AcpiHandler, AcpiTables};

pub unsafe fn init_acpi<H>(acpi: &AcpiTables<H>)
where
	H: AcpiHandler,
{
	disable_pic();
	local_apic::init();
	dbg!(local_apic::get());
	dbg!(local_apic::get() as *const _);
	let info = acpi.platform_info().unwrap();
	dbg!(info.interrupt_model);
}

fn disable_pic() {
	unsafe {
		let b: u8;
		asm!("
			mov {0}, 0xff
			out 0xa1, {0}
			out 0x21, {0}
		", out(reg_byte) b)
	}
}

fn local_apic_address() -> u64 {
	unsafe { msr::rdmsr(msr::IA32_APIC_BASE_MSR) & !(Page::MASK as u64) }
}

fn enable_apic() {
	let v = local_apic_address() | msr::IA32_APIC_BASE_MSR_ENABLE;
	unsafe {
		msr::wrmsr(msr::IA32_APIC_BASE_MSR, v);
	}
}
