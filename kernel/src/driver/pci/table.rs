use crate::object_table::{Table, Query, NoneQuery, Id, Object, CreateObjectError};
use alloc::{boxed::Box, string::String, format};

/// Table with all PCI devices.
pub struct PciTable;

impl Table for PciTable {
	fn name(&self) -> &str {
		"pci"
	}

	fn query(&self, name: Option<&str>, tags: &[&str]) -> Box<dyn Query> {
		if let Some(name) = name {
			Box::new(QueryName { item: bdf_from_string(name) })
		} else { 
			let (mut vendor_id, mut device_id) = (None, None);
			for t in tags {
				let f = |a: &mut Option<u16>, h: &str| {
					u16::from_str_radix(h, 16)
						.ok()
						.and_then(|v| a.replace(v))
						.is_none()
				};
				let r = match t.split_once(':') {
					Some(("vendor-id", h)) => f(&mut vendor_id, h),
					Some(("device-id", h)) => f(&mut device_id, h),
					_ => false,
				};
				if !r {
					return Box::new(NoneQuery);
				}
			}
			Box::new(QueryTags {
				vendor_id,
				device_id,
				index: 0,
			})
		}
	}

	fn get(&self, id: Id) -> Option<Object> {
		let (bus, dev, func) = n_to_bdf(id.into())?;
		let pci = super::PCI.lock();
		let d = pci.as_ref().unwrap().get(bus, dev, func)?;
		Some(pci_dev_object(d, bus, dev, func))
	}

	fn create(&self, _: &str, _: &[&str]) -> Result<Object, CreateObjectError> {
		Err(CreateObjectError { message: "can't create pci devices".into() })
	}
}

struct QueryName {
	item: Option<(u8, u8, u8)>,
}

impl Query for QueryName {}

impl Iterator for QueryName {
	type Item = Object;

	fn next(&mut self) -> Option<Self::Item> {
		dbg!(self.item);
		self.item.take().and_then(|(b, d, f)| {
			let pci = super::PCI.lock();
			let h = pci.as_ref().unwrap().get(b, d, f)?;
			Some(pci_dev_object(h, b, d, f))
		})
	}
}

struct QueryTags {
	vendor_id: Option<u16>,
	device_id: Option<u16>,
	index: u32,
}

impl Query for QueryTags {}

impl Iterator for QueryTags {
	type Item = Object;

	fn next(&mut self) -> Option<Self::Item> {
		let pci = super::PCI.lock();
		let pci = pci.as_ref().unwrap();
		while self.index < 0x100 << 8 {
			let (bus, dev, func) = n_to_bdf(self.index.into()).unwrap();
			self.index += 1;
			if let Some(h) = pci.get(bus, dev, func) {
				if self.vendor_id.map_or(false, |v| v != h.vendor_id()) {
					continue;
				}
				if self.device_id.map_or(false, |v| v != h.device_id()) {
					continue;
				}
				return Some(pci_dev_object(h, bus, dev, func));
			}
		}
		None
	}
}

fn bdf_to_string(bus: u8, dev: u8, func: u8) -> String {
	format!("{}:{:02}.{}", bus, dev, func)
}

fn bdf_from_string(s: &str) -> Option<(u8, u8, u8)> {
	let (bus, s)    = s.split_once(':')?;
	let (dev, func) = s.split_once('.')?;
	Some((bus.parse().ok()?, dev.parse().ok()?, func.parse().ok()?))
}

fn pci_dev_object(h: pci::Header, bus: u8, dev: u8, func: u8) -> Object {
	Object {
		id: (u64::from(bus) << 8 | u64::from(dev) << 3 | u64::from(func)).into(),
		name: bdf_to_string(bus, dev, func).into(),
		tags: [
			format!("vendor-id:{:04x}", h.vendor_id()).into(),
			format!("device-id:{:04x}", h.device_id()).into(),
		].into(),
		interface: Box::new(super::PciDevice::new(bus, dev))
	}
}

fn n_to_bdf(n: u64) -> Option<(u8, u8, u8)> {
	let func = u8::try_from((n >> 0) & 0x07).unwrap();
	let dev  = u8::try_from((n >> 3) & 0x1f).unwrap();
	let bus  = u8::try_from((n >> 8) & 0xff).ok()?;
	Some((bus, dev, func))
}
