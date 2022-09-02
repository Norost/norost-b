cfg_if::cfg_if! {
	if #[cfg(any(target_arch = "x86", target_arch = "x86_64"))] {
		mod x86;
		pub use x86::Uart;
	} else {
		compile_error!("UART not supported on this platform");
	}
}
mod table;

pub use table::UartId;
use {
	crate::{
		object_table,
		sync::spinlock::{AutoGuard, SpinLock},
	},
	alloc::sync::Arc,
	core::fmt,
};

pub static mut DEVICES: [Option<SpinLock<Uart>>; 8] = [const { None }; 8];

/// # Safety
///
/// This function must be called exactly once at boot time.
pub unsafe fn early_init() {
	// This port is guaranteed to exist.
	#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
	unsafe {
		DEVICES[0] = Some(SpinLock::new(Uart::new(0x3f8)));
	}
}

/// # Safety
///
/// This function must be called exactly once at boot time.
pub unsafe fn init() {
	#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
	unsafe {
		x86::init();
	}
}

pub fn post_init(root: &crate::object_table::Root) {
	let table = Arc::new(table::UartTable) as Arc<dyn object_table::Object>;
	root.add(*b"uart", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

/// Acquire a lock on a UART device.
#[cfg_attr(debug_assertions, track_caller)]
#[inline]
pub fn get(i: usize) -> AutoGuard<'static, Uart> {
	// SAFETY: No thread sets DEVICES[i] to None
	unsafe { DEVICES[i].as_ref().unwrap().auto_lock() }
}

/// UART device for emergency situations. This function bypasses the lock and should only be used
/// when things are in an extremely bad state (e.g. double fault).
pub struct EmergencyWriter;

impl fmt::Write for EmergencyWriter {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		unsafe { Uart::new_no_init(0x3f8) }.write_str(s)
	}
}
