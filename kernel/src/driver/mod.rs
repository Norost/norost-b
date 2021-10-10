mod acpi;
pub mod uart;
pub mod vga;

use crate::boot;

pub unsafe fn init(boot: &boot::Info) {
	acpi::init(boot);
}
