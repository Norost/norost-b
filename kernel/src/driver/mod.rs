mod acpi;
pub mod uart;
pub mod vga;
#[cfg(feature = "driver-pci")]
pub mod pci;

use crate::boot;

/// # Safety
///
/// This function may only be called once at boot time
pub unsafe fn init(boot: &boot::Info) {
	acpi::init(boot);
}
