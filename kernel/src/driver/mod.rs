mod acpi;
#[cfg(feature = "driver-pci")]
pub mod pci;
pub mod uart;
pub mod vga;

use crate::boot;

/// # Safety
///
/// This function may only be called once at boot time
pub unsafe fn init(boot: &boot::Info) {
	acpi::init(boot);
}
