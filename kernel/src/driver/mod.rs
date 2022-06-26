mod acpi;
pub mod apic;
#[cfg(feature = "driver-hpet")]
pub mod hpet;
#[cfg(feature = "driver-pci")]
pub mod pci;
#[cfg(feature = "driver-pic")]
pub mod pic;
#[cfg(feature = "driver-portio")]
pub mod portio;
#[cfg(feature = "driver-ps2")]
pub mod ps2;
#[cfg(feature = "driver-rtc")]
pub mod rtc;
pub mod uart;
#[cfg(feature = "driver-vga")]
pub mod vga;

use crate::{boot, object_table::Root};

/// Initialize drivers that are needed very early in the boot process.
///
/// # Safety
///
/// This function may only be called once at boot time
pub unsafe fn early_init(_boot: &boot::Info) {
	unsafe {
		// Initialize UART first as we need it for logging.
		uart::early_init();
	}
}

/// # Safety
///
/// This function must be called exactly once at boot time.
pub unsafe fn init(boot: &boot::Info) {
	// Do not reorder the calls!
	unsafe {
		#[cfg(feature = "driver-vga")]
		vga::init();

		acpi::init(boot);

		#[cfg(feature = "driver-pic")]
		pic::init();

		#[cfg(feature = "driver-rtc")]
		rtc::init();

		uart::init();
	}
}

pub fn post_init(root: &Root) {
	uart::post_init(root);

	#[cfg(feature = "driver-portio")]
	portio::post_init(root);

	#[cfg(feature = "driver-ps2")]
	ps2::post_init(root);

	#[cfg(feature = "driver-ps2")]
	pci::post_init(root);
}
