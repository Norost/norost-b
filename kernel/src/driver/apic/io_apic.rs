use {super::RegRW, crate::memory::r#virtual::phys_to_virt};

#[repr(C)]
struct IoApic {
	index: RegRW,
	data: RegRW,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TriggerMode {
	Edge,
	Level,
}

fn irq_offset(irq: u8) -> u32 {
	0x10 + u32::from(irq) * 2
}

pub unsafe fn set_irq(irq: u8, apic_id: u8, vector: u8, trigger_mode: TriggerMode, mask: bool) {
	let i = irq_offset(irq);

	unsafe {
		// APIC ID | ...
		write(i + 1, read(i + 1) & 0x00ffffff | (u32::from(apic_id) << 24));

		// ... | mask | ... | trigger mode | ... | delivery status | destination | delivery | vector
		let wr = read(i + 0) & 0xfffe_0000;
		let wr = wr | u32::from(mask) << 16;
		let wr = wr
			| match trigger_mode {
				TriggerMode::Edge => 0,
				TriggerMode::Level => 1,
			} << 15;
		let wr = wr | 0 << 12;
		let wr = wr | 0 << 11;
		let wr = wr | 0b000 << 8;
		let wr = wr | u32::from(vector);
		write(i + 0, wr);
	}
}

pub unsafe fn mask_irq(irq: u8, enable: bool) {
	let i = irq_offset(irq);
	unsafe {
		write(i, read(i) & !(1 << 16) | u32::from(enable) << 16);
	}
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
