cfg_if::cfg_if! {
	if #[cfg(any(target_arch = "x86", target_arch = "x86_64"))] {
		mod x86;
		pub use x86::Uart;
	} else {
		compile_error!("UART not supported on this platform");
	}
}
mod table;

use crate::object_table;
use crate::sync::spinlock::{Guard, SpinLock};
use alloc::sync::Arc;
use core::fmt;
pub use table::UartId;

pub static mut DEVICES: [Option<SpinLock<Uart>>; 8] = [const { None }; 8];

/// # Safety
///
/// This function may only be called once at boot time.
pub unsafe fn early_init() {
	// This port is guaranteed to exist.
	#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
	unsafe {
		DEVICES[0] = Some(SpinLock::new(Uart::new(0x3f8)));
	}
}

/// # Safety
///
/// This function may only be called once after [`init`] and when the APIC is initialized.
pub unsafe fn post_init(root: &crate::object_table::Root) {
	#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
	unsafe {
		x86::init();
	}

	let table = Arc::new(table::UartTable) as Arc<dyn object_table::Object>;
	root.add(*b"uart", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

/// Acquire a lock on a UART device.
pub fn get(i: usize) -> Guard<'static, Uart> {
	// SAFETY: No thread sets DEVICES[i] to None
	unsafe { DEVICES[i].as_ref().unwrap().lock() }
}

/// UART device for emergency situations. This function bypasses the lock and should only be used
/// when things are in an extremely bad state (e.g. double fault).
pub struct EmergencyWriter;

impl fmt::Write for EmergencyWriter {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		unsafe { Uart::new_no_init(0x3f8) }.write_str(s)
	}
}
