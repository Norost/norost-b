//! Driver for iterating and interacting with PCI and PCIe devices.
//!
//! ## References
//!
//! [PCI on OSDev wiki][osdev pci]
//!
//! [osdev pci]: https://wiki.osdev.org/PCI

mod mcfg;

use {
	crate::{
		driver::apic::local_apic,
		memory::r#virtual::phys_to_virt,
		object_table::{self, Root},
		sync::SpinLock,
	},
	acpi::{sdt::Signature, AcpiHandler, AcpiTables},
	alloc::sync::Arc,
	core::ptr::NonNull,
	pci::{
		capability::{Capability, Msi, MsiX},
		Pci,
	},
};

mod device;
mod table;

pub use device::PciDevice;

static PCI: SpinLock<Option<Pci>> = SpinLock::new(None);

/// # Safety
///
/// This function must be called exactly once at boot time.
pub(super) unsafe fn init_acpi<H>(acpi: &AcpiTables<H>)
where
	H: AcpiHandler,
{
	let mcfg = unsafe {
		match acpi.get_sdt::<mcfg::Mcfg>(Signature::MCFG) {
			Ok(Some(p)) => p,
			Ok(None) => return warn!("MCFG not found"),
			Err(e) => return warn!("Failed to parse MCFG: {:?}", e),
		}
	};

	let e = match mcfg.entries() {
		[] => return warn!("No MCFG entries"),
		[e] => e,
		[e, ..] => {
			warn!("Ignoring extra MCFG entries");
			e
		}
	};

	assert_eq!(e.bus_number_start, 0, "todo: very funny PCI thing");

	let phys = e.base_address();
	let size = (usize::from(e.bus_number_end) + 1) * 32 * 8 * 4096;
	let virt = unsafe { NonNull::new(phys_to_virt(phys)).unwrap() };

	let mut pci = unsafe { Pci::new(virt.cast(), phys.try_into().unwrap(), size, &[]) };

	unsafe {
		allocate_irqs(&mut pci);
	}

	*PCI.isr_lock() = Some(pci);
}

pub(super) fn post_init(root: &Root) {
	let table = Arc::new(table::PciTable) as Arc<dyn object_table::Object>;
	root.add(*b"pci", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

/// Allocate IRQs for all PCI devices that need it.
/// While there are "only" ~220 available IRQs, this should be enough for the foreseeable future.
unsafe fn allocate_irqs(pci: &mut Pci) {
	for dev in pci.iter().flat_map(|b| b.iter()) {
		let h = dev.header();
		let mut cmd = h.common().command();
		cmd &= !pci::HeaderCommon::COMMAND_INTERRUPT_DISABLE;
		cmd |= pci::HeaderCommon::COMMAND_MMIO_MASK;
		cmd |= pci::HeaderCommon::COMMAND_BUS_MASTER_MASK;
		h.set_command(cmd);
		enum Int<'a> {
			None,
			Msi(&'a Msi),
			MsiX(&'a MsiX),
		}
		let mut int = Int::None;
		for cap in h.capabilities() {
			match cap.downcast() {
				Some(Capability::Msi(msi)) => int = Int::Msi(msi),
				Some(Capability::MsiX(msix)) => {
					int = Int::MsiX(msix);
					break;
				}
				_ => {}
			}
		}

		let alloc_irq;
		#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
		alloc_irq = || {
			use crate::arch::amd64;
			let irq = amd64::allocate_irq().expect("irq");
			unsafe {
				amd64::set_interrupt_handler(irq.into(), irq_handler);
			}
			irq
		};

		let (bus, dev, func) = (dev.bus(), dev.device(), 0);
		match int {
			Int::None => info!("No MSI or MSI-X for {:02x}:{:02x}.{:x}", bus, dev, func),
			Int::Msi(msi) => {
				let mut ctrl = msi.message_control();
				let mmc = ctrl.multiple_message_capable().expect("invalid MMC");
				// Limit to 1 for now, as the IRQ allocator is quite primitive.
				// (It may be worth leaving it like this, modern hardware uses MSI-X anyways).
				info!(
					"{} (max: 1) MSI vectors for {:02x}:{:02x}.{:x}",
					(1 << mmc as u8),
					bus,
					dev,
					func
				);

				ctrl.set_multiple_message_enable(pci::capability::MsiInterrupts::N1);
				let irq = alloc_irq();
				msi.set_message_data(irq.into());
				msi.set_message_address(local_apic::get_phys());

				ctrl.set_enable(true);
				msi.set_message_control(ctrl);
			}
			Int::MsiX(msix) => {
				let mut ctrl = msix.message_control();
				let table_size = usize::from(ctrl.table_size()) + 1;
				info!(
					"{} MSI-X vectors for {:02x}:{:02x}.{:x}",
					table_size, bus, dev, func
				);

				let (table_offset, table) = msix.table();
				let table = h.full_base_address(table.into()).expect("bar");
				let table = table.try_as_mmio().expect("mmio bar") + u64::from(table_offset);

				let table = unsafe { phys_to_virt(table) };
				let table = unsafe {
					core::slice::from_raw_parts_mut(
						table.cast::<pci::msix::TableEntry>(),
						table_size,
					)
				};

				for e in table.iter_mut() {
					let irq = alloc_irq();
					e.set_message_data(irq.into());
					e.set_message_address(local_apic::get_phys());
					e.set_vector_control_mask(false);
				}

				ctrl.set_enable(true);
				msix.set_message_control(ctrl);
			}
		}
	}
}

extern "C" fn irq_handler(_: u32) {
	device::irq_handler();
	local_apic::get().eoi.set(0);
}
