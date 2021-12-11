use super::{RegR, RegW, RegRW};
use crate::memory::frame::PPN;
use crate::memory::r#virtual::{AddressSpace, phys_to_virt};

#[repr(C)]
struct IoApic {
	index: RegRW,
	data: RegRW,
}

pub fn set_irq(irq: u8, apic_id: u8, vector: u8) {
	let i = 0x10 + u32::from(irq) * 2;

	unsafe {
		// APIC ID | ...
		write(i + 1, read(i + 1) & 0x00ffffff | (u32::from(apic_id) << 24));

		// ... | mask | ... | delivery status | destination | delivery | vector
		write(i + 0, read(i + 0) & 0xfffe_0000 | 0 << 12 | 0 << 11 | 0b000 << 8 | u32::from(vector))
	}
}

pub(super) fn init() {
	// Ensure the I/O APIC registers are mapped.
	let a = PPN::try_from_usize(io_apic_address().try_into().unwrap()).unwrap();
	AddressSpace::identity_map(a, 4096);
	super::enable_apic();
}

/// Read a register from the IoApic
///
/// # Safety
///
/// The register must be valid.
unsafe fn read(index: u32) -> u32 {
	let apic = io_apic();
	apic.index.set(index);
	apic.data.get()
}

/// Write to a register of the IoApic
///
/// # Safety
///
/// The register must be valid.
unsafe fn write(index: u32, value: u32) {
	let apic = io_apic();
	apic.index.set(index);
	apic.data.set(value);
}

/// Get a reference to the IoApic.
fn io_apic() -> &'static IoApic {
	unsafe { &*phys_to_virt(io_apic_address().try_into().unwrap()).cast() }
}

/// Get the physical pointer to the I/O APIC
fn io_apic_address() -> usize {
	// TODO don't hardcode the address
	0xfec00000
}
