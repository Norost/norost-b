use crate::object_table::{Table, Query, Id, Object, CreateObjectError};
use alloc::{boxed::Box, string::String, format};

/// Table with all PCI devices.
pub struct PciTable;

impl Table for PciTable {
	fn name(&self) -> &str {
		"pci"
	}

	fn query(&self, name: Option<&str>, tags: &[&str]) -> Box<dyn Query> {
		Box::new(PciQuery { item: from_string(name.unwrap_or("")) })
	}

	fn get(&self, id: Id) -> Option<Object> {
		let num  = u64::from(id);
		let func = u8::try_from((num >> 0) & 0x07).unwrap();
		let dev  = u8::try_from((num >> 3) & 0x1f).unwrap();
		let bus  = u8::try_from((num >> 8) & 0xff).ok()?;
		let pci  = super::PCI.lock();
		let _    = pci.as_ref().unwrap().get(bus, dev, func)?;
		Some(pci_dev_object(bus, dev, func))
	}

	fn create(&self, _: &str, _: &[&str]) -> Result<Object, CreateObjectError> {
		Err(CreateObjectError { message: "can't create pci devices".into() })
	}
}

struct PciQuery {
	item: Option<(u8, u8, u8)>,
}

impl Query for PciQuery {}

impl Iterator for PciQuery {
	type Item = Object;

	fn next(&mut self) -> Option<Self::Item> {
		self.item.take().and_then(|(b, d, f)| {
			let _ = super::PCI.lock().as_ref().unwrap().get(b, d, f)?;
			Some(pci_dev_object(b, d, f))
		})
	}
}

fn to_string(bus: u8, dev: u8, func: u8) -> String {
	format!("{}:{:02}.{}", bus, dev, func)
}

fn from_string(s: &str) -> Option<(u8, u8, u8)> {
	let (bus, s)    = s.split_once(':')?;
	let (dev, func) = s.split_once('.')?;
	Some((bus.parse().ok()?, dev.parse().ok()?, func.parse().ok()?))
}

fn pci_dev_object(bus: u8, dev: u8, func: u8) -> Object {
	Object {
		id: (u64::from(bus) << 8 | u64::from(dev) << 3 | u64::from(func)).into(),
		name: to_string(bus, dev, func).into(),
		tags: [].into(),
		interface: Box::new(super::PciDevice::new(bus, dev))
	}
}
