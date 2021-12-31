pub mod io_apic;
pub mod local_apic;
mod reg;

use crate::arch::amd64::msr;
use crate::memory::Page;
use crate::time::Monotonic;
use acpi::{AcpiHandler, AcpiTables};
use core::arch::asm;
use core::time::Duration;
use reg::*;

// No atomic is necessary as the value is read only once anyways.
static mut TICKS_PER_SECOND: u32 = 0;

pub unsafe fn init_acpi<H>(_: &AcpiTables<H>)
where
	H: AcpiHandler,
{
	disable_pic();
	local_apic::init();
	io_apic::init();
}

pub fn post_init() {
	// Set interrupt vector
	let t = local_apic::get().lvt_timer.get();
	local_apic::get().lvt_timer.set(t & !0xff | 61);

	calibrate_timer(Duration::from_millis(10));
}

/// Set the timer in one-shot mode for the given duration in the future.
///
/// Smaller durations are more precise. The timer may end early if the duration
/// is too large.
pub fn set_timer_oneshot(t: Duration, irq: Option<u8>) {
	let mut ticks = t
		.as_nanos()
		.saturating_mul(unsafe { TICKS_PER_SECOND }.into())
		.saturating_div(1_000_000_000);
	// Scale down the resolution until the ticks fit
	let mut shift = 0;
	let ticks = loop {
		if let Ok(ticks) = ticks.try_into() {
			break ticks;
		}
		ticks >>= 1;
		shift += 1;
	};
	// Translate shift to something we can put in the divide configuration reigster
	let (shift, ticks) = match shift {
		0 => (0b1011, ticks),
		1 => (0b0000, ticks),
		2 => (0b1000, ticks),
		3 => (0b0010, ticks),
		4 => (0b1010, ticks),
		5 => (0b0001, ticks),
		6 => (0b1001, ticks),
		7 => (0b0011, ticks),
		_ => (0b0011, u32::MAX), // Default to highest
	};

	if let Some(irq) = irq {
		let t = local_apic::get().lvt_timer.get();
		local_apic::get()
			.lvt_timer
			.set(t & !(1 << 16 | 0xff) | u32::from(irq));
	}

	local_apic::get().divide_configuration.set(shift);
	local_apic::get().initial_count.set(ticks);
}

/// Loop for the given duration and count the amount of passed ACPI timer cycles to
/// calibrate the timer.
fn calibrate_timer(t: Duration) {
	let end = Monotonic::now().saturating_add(t);
	let lapic = local_apic::get();
	lapic.divide_configuration.set(0b1011); // Set divisor to 1
	lapic.initial_count.set(u32::MAX);
	while Monotonic::now() < end { /* pass */ }
	let ticks = u32::MAX - lapic.current_count.get();
	lapic.initial_count.set(0);
	unsafe {
		TICKS_PER_SECOND = u128::from(ticks)
			.checked_mul(1_000_000_000)
			.expect("multiplication overflow")
			.checked_div(t.as_nanos())
			.expect("division overflow")
			.try_into()
			.expect("too many ticks per second");
	}
}

fn disable_pic() {
	unsafe {
		asm!("
			mov al, 0xff
			out 0x21, al
			out 0xa1, al
		", out("al") _)
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
