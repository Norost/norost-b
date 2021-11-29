use super::PCI;
use crate::memory::frame::PageFrame;
use crate::memory::frame::PPN;
use core::ptr::NonNull;
use pci::{BaseAddress, Header};

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

	pub fn bar_region(&self, index: u8) -> Result<PageFrame, BarError> {
		let index = usize::from(index);
		let pci = PCI.lock();
		let pci = pci.as_ref().unwrap();
		let header = pci.get(self.bus, self.device, 0).unwrap();
		match header {
			Header::H0(h) => {
				h.base_address
					.get(index)
					.ok_or(BarError::NonExistent)
					.and_then(|bar| {
						let (size, orig) = bar.size();
						bar.set(orig);
						if let Some(size) = size {
							if !BaseAddress::is_mmio(orig) {
								return Err(BarError::NotMmio);
							}
							let upper = || h.base_address.get(index + 1).map(|e| e.get());
							let addr = BaseAddress::address(orig, upper).unwrap();
							Ok(PageFrame {
								base: PPN::try_from_usize(addr.try_into().unwrap()).unwrap(),
								p2size: size.trailing_zeros() as u8 - 12, // log2
							})
						} else {
							Err(BarError::Invalid)
						}
					})
			}
			_ => todo!(),
		}
	}
}

#[derive(Debug)]
pub enum BarError {
	NonExistent,
	Invalid,
	NotMmio,
}
