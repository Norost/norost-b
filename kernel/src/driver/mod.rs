mod acpi;
pub mod apic;
#[cfg(feature = "driver-hpet")]
pub mod hpet;
#[cfg(feature = "driver-pci")]
pub mod pci;
#[cfg(feature = "driver-pic")]
pub mod pic;
pub mod rtc;
pub mod uart;
#[cfg(feature = "driver-vga")]
pub mod vga;

use crate::boot;

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
/// This function may only be called once at boot time
pub unsafe fn init(boot: &boot::Info, root: &crate::object_table::Root) {
	// Do not reorder the calls!
	unsafe {
		#[cfg(feature = "driver-vga")]
		vga::init();

		acpi::init(boot, root);

		#[cfg(feature = "driver-pic")]
		pic::init();

		rtc::init();

		apic::post_init();

		uart::post_init(root);
	}
}
