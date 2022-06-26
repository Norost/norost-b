use crate::{
	boot,
	memory::r#virtual::{phys_to_virt, virt_to_phys},
};

#[derive(Clone, Debug)]
struct Handler;

impl acpi::AcpiHandler for Handler {
	unsafe fn map_physical_region<T>(
		&self,
		phys: usize,
		size: usize,
	) -> acpi::PhysicalMapping<Self, T> {
		unsafe {
			let virt = core::ptr::NonNull::new_unchecked(phys_to_virt(phys.try_into().unwrap()));
			acpi::PhysicalMapping::new(phys, virt.cast(), size, size, Handler)
		}
	}

	fn unmap_physical_region<T>(_: &acpi::PhysicalMapping<Self, T>) {}
}

/// # Safety
///
/// This function must be called exactly once at boot time.
pub unsafe fn init(boot: &boot::Info) {
	boot.rsdp.validate().unwrap();

	unsafe {
		let rsdp = virt_to_phys(&boot.rsdp as *const _ as *const _)
			.try_into()
			.unwrap();
		let acpi = acpi::AcpiTables::from_rsdp(Handler, rsdp).unwrap();

		super::apic::init_acpi(&acpi);

		#[cfg(feature = "driver-ps2")]
		super::ps2::init_acpi(&acpi);

		#[cfg(feature = "driver-pci")]
		super::pci::init_acpi(&acpi);

		#[cfg(feature = "driver-hpet")]
		super::hpet::init_acpi(&acpi);

		// Calibrate at end since we need another timer for this.
		super::apic::post_init_acpi();
	}
}
