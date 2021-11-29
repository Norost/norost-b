use super::PciDevice;
use super::PCI;
use crate::scheduler::process::Process;
use crate::scheduler::syscall::Return;

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
			.pci_add_device(dev, address as *const _)
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
	Process::current()
		.pci_map_bar(handle as u32, bar as u8, address as *mut _)
		.unwrap();
	Return {
		status: 0,
		value: 0,
	}
}
