//! # RTC driver
use core::arch::asm;

use crate::time::Monotonic;
use core::sync::atomic::AtomicU64;

static RTC_TICKS: AtomicU64 = AtomicU64::new(0);

const RTC_IRQ: usize = 32 + 8;
const RTC_RATE: u8 = 15;

impl Monotonic {
	#[cfg(not(feature = "driver-hpet"))]
	pub fn now() -> Self {
		// Frequency (Hz) is `32768 >> (rate - 1)`, default rate is 6
		let freq = 1 << (16 - RTC_RATE);
		Self::from_seconds((RTC_TICKS.load(Ordering::Relaxed) / freq).into())
	}
}

#[naked]
pub(super) extern "C" fn irq() {
	// SAFETY: no registers are clobbered and the reads & writes are to valid
	// static addresses only _and_ are atomic.
	unsafe {
		asm!("
			push	rax

			# Since only a single core should be handling the RTC interrupt at any time
			# it should be fine to _not_ use a lock prefix, as there is one writer only
			# anyways (mov loads are always atomic).
			inc		DWORD PTR [rip + {rtc_ticks}]

			# Read register C to ensure interrupts will happen again.
			mov		al, 0xc
			out		0x70, al
			in		al, 0x71

			# Mark EOI
			movabs	rax, {eoi_addr}
			mov		DWORD PTR [rax], 0

			pop		rax
			iretq
			",
			rtc_ticks = sym RTC_TICKS,
			eoi_addr  = const 0xffff_c000_fee0_00b0u64,
			options(noreturn),
		);
	}
}

pub(super) fn init() {
	unsafe {
		use crate::arch::amd64::{idt_set, Handler, IDTEntry};
		idt_set(RTC_IRQ, IDTEntry::new(1 * 8, Handler::Int(irq), 0));
		asm!("
			# Disable interrupts
			pushf
			cli

			# Select register B, disable NMI & read it
			mov		al, 0x8b
			out 	0x70, al
			in		al, 0x71
			# Enable IRQs
			or		al, 1 << 6
			push	rax
			# Select register B again & write it
			mov		al, 0x8b
			out		0x70, al
			pop		rax
			out		0x71, al

			# Select A & set rate
			mov		al, 0x8a
			out		0x70, al
			in		al, 0x71
			and		al, 0xf0
			or		al, {rate}
			push	rax
			mov		al, 0x8a
			out		0x70, al
			pop		rax
			out		0x71, al

			# Restore interrupts (if they were enabled)
			popf

			# Ensure register C is clear so interrupts will be sent.
			mov		al, 0xc
			out		0x70, al
			in		al, 0x71
			",
			rate = const RTC_RATE,
			lateout("rax") _,
		);
	}
}
