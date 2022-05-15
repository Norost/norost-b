use crate::object_table::{Error, NoneQuery, Object, Query, QueryResult, Ticket};
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use core::{mem, str};

/// Table with all PCI devices.
pub struct PciTable;

impl Object for PciTable {
	fn query(self: Arc<Self>, prefix: Vec<u8>, path: &[u8]) -> Ticket<Box<dyn Query>> {
		let (mut vendor_id, mut device_id) = (None, None);
		for t in path.split(|c| *c == b'&') {
			let f = |a: &mut Option<u16>, h: &[u8]| {
				let n = u16::from_str_radix(str::from_utf8(h).ok()?, 16).ok()?;
				if a.is_some() && *a != Some(n) {
					None
				} else {
					*a = Some(n);
					Some(())
				}
			};
			match t
				.iter()
				.position(|c| *c == b':')
				.map(|i| t.split_at(i.into()))
			{
				Some((b"vendor-id", h)) => f(&mut vendor_id, &h[1..]),
				Some((b"device-id", h)) => f(&mut device_id, &h[1..]),
				Some((b"name", h)) => {
					// Names are unique
					return Ticket::new_complete(Ok(str::from_utf8(&h[1..]).map_or(
						Box::new(NoneQuery),
						|h| {
							Box::new(QueryName {
								prefix,
								item: bdf_from_string(h),
							})
						},
					)));
				}
				_ => None,
			};
		}
		Ticket::new_complete(Ok(Box::new(QueryTags {
			prefix,
			vendor_id,
			device_id,
			index: 0,
		})))
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(
			path_to_bdf(path)
				.and_then(|(bus, dev, func)| {
					let pci = super::PCI.auto_lock();
					pci.as_ref()
						.unwrap()
						.get(bus, dev, func)
						.map(|d| pci_dev_object(d, bus, dev, func))
						.map(Ok)
				})
				.unwrap_or_else(|| Err(Error::DoesNotExist)),
		)
	}
}

struct QueryName {
	prefix: Vec<u8>,
	item: Option<(u8, u8, u8)>,
}

impl Query for QueryName {}

impl Iterator for QueryName {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		self.item.take().and_then(|(b, d, f)| {
			let pci = super::PCI.lock();
			pci.as_ref().unwrap().get(b, d, f)?;
			let path = mem::take(&mut self.prefix);
			Some(Ticket::new_complete(Ok(pci_dev_query_result(
				path, b, d, f,
			))))
		})
	}
}

struct QueryTags {
	prefix: Vec<u8>,
	vendor_id: Option<u16>,
	device_id: Option<u16>,
	index: u32,
}

impl Query for QueryTags {}

impl Iterator for QueryTags {
	type Item = Ticket<QueryResult>;

	fn next(&mut self) -> Option<Self::Item> {
		let pci = super::PCI.auto_lock();
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
				return Some(Ticket::new_complete(Ok(pci_dev_query_result(
					self.prefix.clone(),
					bus,
					dev,
					func,
				))));
			}
		}
		None
	}
}

fn bdf_to_string(bus: u8, dev: u8, func: u8) -> String {
	format!("{}:{:02}.{}", bus, dev, func)
}

fn bdf_from_string(s: &str) -> Option<(u8, u8, u8)> {
	let (bus, s) = s.split_once(':')?;
	let (dev, func) = s.split_once('.')?;
	Some((bus.parse().ok()?, dev.parse().ok()?, func.parse().ok()?))
}

fn pci_dev_object(_h: pci::Header, bus: u8, dev: u8, _func: u8) -> Arc<dyn Object> {
	Arc::new(super::PciDevice::new(bus, dev))
}

fn pci_dev_query_result(mut path: Vec<u8>, bus: u8, dev: u8, func: u8) -> QueryResult {
	path.extend(bdf_to_string(bus, dev, func).into_bytes());
	QueryResult { path: path.into() }
}

fn n_to_bdf(n: u64) -> Option<(u8, u8, u8)> {
	let func = u8::try_from((n >> 0) & 0x07).unwrap();
	let dev = u8::try_from((n >> 3) & 0x1f).unwrap();
	let bus = u8::try_from((n >> 8) & 0xff).ok()?;
	Some((bus, dev, func))
}

fn path_to_bdf(path: &[u8]) -> Option<(u8, u8, u8)> {
	let path = core::str::from_utf8(path).ok()?;
	let (bus, dev_func) = path.split_once(':')?;
	let (dev, func) = dev_func.split_once('.')?;
	Some((bus.parse().ok()?, dev.parse().ok()?, func.parse().ok()?))
}
