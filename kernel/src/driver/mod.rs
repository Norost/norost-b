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

/// # Safety
///
/// This function may only be called once at boot time
pub unsafe fn init(boot: &boot::Info) {
	// Do not reorder the calls!
	uart::init();

	acpi::init(boot);

	#[cfg(feature = "driver-pic")]
	pic::init();

	rtc::init();

	apic::post_init();
}
