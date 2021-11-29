use super::PciDevice;
use super::PCI;
use crate::memory::r#virtual::{RWX, MemoryObjectHandle};
use crate::scheduler::process::Process;
use crate::scheduler::syscall::Return;
use core::ptr::NonNull;
use alloc::boxed::Box;

pub extern "C" fn map_any(
	id: usize,
	address: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let vendor = (id >> 16) as u16;
	let device = id as u16;

	let mut pci_lock = PCI.lock();
	let pci = pci_lock.as_mut().unwrap();

	let mut bus_dev = None;

	'outer: for bus in pci.iter() {
		for dev in bus.iter() {
			// Read both so that (hopefully) the compiler turns it into one
			// volatile load
			let (v, d) = (dev.vendor_id(), dev.device_id());
			dev.header().set_command(
				pci::HeaderCommon::COMMAND_MMIO_MASK
					| pci::HeaderCommon::COMMAND_BUS_MASTER_MASK,
			);
			if v == vendor && d == device {
				dev.header().set_command(
					pci::HeaderCommon::COMMAND_MMIO_MASK
						| pci::HeaderCommon::COMMAND_BUS_MASTER_MASK,
				);
				bus_dev = Some((dev.bus(), dev.device()));
				break 'outer;
			}
		}
	}
	drop(pci_lock);

	if let Some((bus, dev)) = bus_dev {
		let dev = PciDevice::new(bus, dev);
		let handle = Process::current()
			.map_memory_object(NonNull::new(address as *mut _), Box::new(dev), RWX::R)
			.unwrap();
		Return {
			status: 0,
			value: handle.try_into().unwrap(),
		}
	} else {
		Return {
			status: 1,
			value: 0,
		}
	}
}

pub extern "C" fn map_bar(
	handle: usize,
	bar: usize,
	address: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let handle = MemoryObjectHandle::from(handle);
	let p = Process::current();
	let dev = p.get_memory_object(handle)
		.and_then(|d| {
			<dyn core::any::Any>::downcast_ref::<PciDevice>(d)
		})
		.unwrap();
	let bar = dev.bar_region(bar.try_into().unwrap()).unwrap();
	p.map_memory_object(NonNull::new(address as *mut _), Box::new(bar), RWX::RW)
		.unwrap();
	Return {
		status: 0,
		value: 0,
	}
}
