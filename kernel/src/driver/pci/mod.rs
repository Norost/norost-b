//! Driver for iterating and interacting with PCI and PCIe devices.
//!
//! ## References
//!
//! [PCI on OSDev wiki][osdev pci]
//!
//! [osdev pci]: https://wiki.osdev.org/PCI

use crate::driver::apic::local_apic;
use crate::memory::r#virtual::{add_identity_mapping, phys_to_virt};
use crate::object_table;
use crate::sync::SpinLock;
use acpi::{AcpiHandler, AcpiTables, PciConfigRegions};
use alloc::sync::Arc;
use core::ptr::NonNull;
use pci::Pci;

mod device;
mod table;

pub use device::PciDevice;

static PCI: SpinLock<Option<Pci>> = SpinLock::new(None);

pub unsafe fn init_acpi<H>(acpi: &AcpiTables<H>, root: &crate::object_table::Root)
where
	H: AcpiHandler,
{
	let pci = match PciConfigRegions::new(acpi) {
		Ok(p) => p,
		Err(e) => {
			warn!("failed to load PCI configuration regions: {:?}", e);
			return;
		}
	};
	let mut avail = [0u128; 2];
	// TODO this is ridiculous. Fork the crate or implement MCFG ourselves.
	for bus in 0..=255 {
		// IDK what a segment group is
		if pci.physical_address(0, bus, 0, 0).is_some() {
			avail[usize::from(bus >> 7)] |= 1 << (bus & 0x7f);
		}
	}
	assert_eq!(avail, [u128::MAX; 2], "todo: handle PCI bus stupidity");

	let phys = pci.physical_address(0, 0, 0, 0).unwrap();
	let size = 256 * 32 * 8 * 4096;
	unsafe { add_identity_mapping(phys.try_into().unwrap(), size).unwrap() };
	let virt = unsafe { NonNull::new(phys_to_virt(phys.try_into().unwrap())).unwrap() };

	let mut pci = unsafe { Pci::new(virt.cast(), phys.try_into().unwrap(), size, &[]) };

	unsafe {
		allocate_irqs(&mut pci);
	}

	*PCI.auto_lock() = Some(pci);

	let table = Arc::new(table::PciTable) as Arc<dyn object_table::Object>;
	root.add(*b"pci", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

/// Allocate IRQs for all PCI devices that need it.
/// While there are "only" ~220 available IRQs, this should be enough for the foreseeable future.
unsafe fn allocate_irqs(pci: &mut Pci) {
	for dev in pci.iter().flat_map(|b| b.iter()) {
		let h = dev.header();
		let cmd = h.common().command();
		h.set_command(cmd | pci::HeaderCommon::COMMAND_BUS_MASTER_MASK);
		for cap in h.capabilities() {
			use pci::capability::Capability;
			match cap.downcast() {
				Some(Capability::MsiX(msix)) => {
					let mut ctrl = msix.message_control();
					let table_size = usize::from(ctrl.table_size()) + 1;

					let (table_offset, table) = msix.table();
					let table = h.full_base_address(table.into()).expect("bar");
					let table = table.try_as_mmio().expect("mmio bar") + u64::from(table_offset);

					let (pending_offset, pending) = msix.pending();
					let pending = h.full_base_address(pending.into()).expect("bar");
					let pending =
						pending.try_as_mmio().expect("mmio bar") + u64::from(pending_offset);

					use crate::memory::frame::PPN;
					use crate::memory::r#virtual::AddressSpace;

					let ppn = PPN::try_from_usize((table & !0xfff).try_into().unwrap()).unwrap();
					AddressSpace::identity_map(ppn, 4096).unwrap();
					let table = unsafe { phys_to_virt(table) };
					let table = unsafe {
						core::slice::from_raw_parts_mut(
							table.cast::<pci::msix::TableEntry>(),
							table_size,
						)
					};

					let pending_ppn =
						PPN::try_from_usize((pending & !0xfff).try_into().unwrap()).unwrap();
					if pending_ppn != ppn {
						AddressSpace::identity_map(pending_ppn, 4096).unwrap();
					}

					for e in table.iter_mut() {
						let irq;
						#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
						irq = {
							use crate::arch::amd64;
							let irq = amd64::allocate_irq().expect("irq");
							unsafe {
								amd64::idt_set(irq.into(), crate::wrap_idt!(irq_handler));
							}
							irq
						};

						e.set_message_data(irq.into());
						e.set_message_address(local_apic::get_phys());
						e.set_vector_control_mask(false);
					}

					ctrl.set_enable(true);
					msix.set_message_control(ctrl);
				}
				_ => {}
			}
		}
	}
}

extern "C" fn irq_handler() {
	device::irq_handler();
	local_apic::get().eoi.set(0);
}
