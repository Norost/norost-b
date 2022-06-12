/// Halt forever. Used to implement sleep.
#[naked]
pub extern "C" fn halt_forever() -> ! {
	unsafe {
		core::arch::asm!(
			"2:",
			"sti",
			"hlt",
			"cli",
			"int {}",
			"jmp 2b",
			const super::TIMER_IRQ,
			options(noreturn)
		)
	}
}

pub fn yield_current_thread() {
	unsafe {
		debug_assert!(
			super::interrupts_enabled(),
			"can't yield while interrupts are disabled"
		);
		// Fake timer interrupt
		core::arch::asm!(
			"int {}",
			const super::TIMER_IRQ,
			options(nomem, nostack, preserves_flags)
		)
	}
}
