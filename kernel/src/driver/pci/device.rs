use super::PCI;
use crate::memory::frame::PageFrame;
use crate::memory::frame::PPN;
use crate::object_table::Object;
use crate::scheduler::MemoryObject;
use alloc::boxed::Box;
use pci::BaseAddress;

/// A single PCI device.
pub struct PciDevice {
	bus: u8,
	device: u8,
}

impl PciDevice {
	pub(super) fn new(bus: u8, device: u8) -> Self {
		Self { bus, device }
	}

	pub fn config_region(&self) -> PageFrame {
		let pci = PCI.lock();
		let pci = pci.as_ref().unwrap();
		let addr = pci.get_physical_address(self.bus, self.device, 0);
		PageFrame {
			base: PPN::try_from_usize(addr).unwrap(),
			p2size: 0,
		}
	}
}

impl MemoryObject for PciDevice {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		[self.config_region()].into()
	}
}

impl Object for PciDevice {
	fn memory_object(&self, offset: u64) -> Option<Box<dyn MemoryObject>> {
		if offset == 0 {
			return Some(Box::new(PciDevice {
				device: self.device,
				bus: self.bus,
			}));
		}

		let index = usize::try_from(offset - 1).ok()?;
		let pci = PCI.lock();
		let pci = pci.as_ref().unwrap();
		let header = pci.get(self.bus, self.device, 0).unwrap();
		let bar = header.base_addresses().get(index)?;
		let (size, orig) = bar.size();
		bar.set(orig);
		let size = size?;
		if !BaseAddress::is_mmio(orig) {
			return None;
		}
		let upper = || header.base_addresses().get(index + 1).map(|e| e.get());
		let addr = BaseAddress::address(orig, upper).unwrap();
		let frame = PageFrame {
			base: PPN::try_from_usize(addr.try_into().unwrap()).unwrap(),
			p2size: size.trailing_zeros() as u8 - 12, // log2
		};
		Some(Box::new(BarRegion { frame }))
	}
}

/// A single MMIO region pointer to by a BAR of a PCI device.
pub struct BarRegion {
	frame: PageFrame,
}

impl MemoryObject for BarRegion {
	fn physical_pages(&self) -> Box<[PageFrame]> {
		[self.frame].into()
	}
}
