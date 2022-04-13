use crate::object_table::{
	Data, Error, Id, Job, JobTask, NoneQuery, Object, Query, QueryResult, Table, Ticket,
};
use alloc::{boxed::Box, format, string::String, string::ToString, sync::Arc};
use core::str;

/// Table with all PCI devices.
pub struct PciTable;

impl Table for PciTable {
	fn name(&self) -> &str {
		"pci"
	}

	fn query(self: Arc<Self>, path: &[u8]) -> Box<dyn Query> {
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
			let _ = dbg!(core::str::from_utf8(t));
			let r = match t
				.iter()
				.position(|c| *c == b':')
				.map(|i| t.split_at(i.into()))
			{
				Some((b"vendor-id", h)) => f(&mut vendor_id, &h[1..]),
				Some((b"device-id", h)) => f(&mut device_id, &h[1..]),
				Some((b"name", h)) => {
					// Names are unique
					return str::from_utf8(&h[1..]).map_or(Box::new(NoneQuery), |h| {
						Box::new(QueryName {
							item: bdf_from_string(h),
						})
					});
				}
				_ => None,
			};
			dbg!(&r, &vendor_id, &device_id);
			if r.is_none() {
				return Box::new(NoneQuery);
			}
		}
		Box::new(QueryTags {
			vendor_id,
			device_id,
			index: 0,
		})
	}

	fn get(self: Arc<Self>, id: Id) -> Ticket {
		let r = n_to_bdf(id.into())
			.and_then(|(bus, dev, func)| {
				let pci = super::PCI.lock();
				pci.as_ref()
					.unwrap()
					.get(bus, dev, func)
					.map(|d| Data::Object(pci_dev_object(d, bus, dev, func)))
			})
			.ok_or_else(|| todo!());
		Ticket::new_complete(r)
	}

	fn create(self: Arc<Self>, _: &[u8]) -> Ticket {
		let e = Error {
			code: 1,
			message: "can't create pci devices".into(),
		};
		Ticket::new_complete(Err(e))
	}

	fn take_job(self: Arc<Self>, _: core::time::Duration) -> JobTask {
		unreachable!("kernel only table")
	}

	fn finish_job(self: Arc<Self>, _: Job) -> Result<(), ()> {
		unreachable!("kernel only table")
	}

	fn cancel_job(self: Arc<Self>, _: Job) {
		unreachable!("kernel only table")
	}
}

impl Object for PciTable {}

struct QueryName {
	item: Option<(u8, u8, u8)>,
}

impl Query for QueryName {}

impl Iterator for QueryName {
	type Item = QueryResult;

	fn next(&mut self) -> Option<Self::Item> {
		self.item.take().and_then(|(b, d, f)| {
			let pci = super::PCI.lock();
			let h = pci.as_ref().unwrap().get(b, d, f)?;
			Some(pci_dev_query_result(h, b, d, f))
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
	type Item = QueryResult;

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
				return Some(pci_dev_query_result(h, bus, dev, func));
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

fn pci_dev_query_result(h: pci::Header, bus: u8, dev: u8, func: u8) -> QueryResult {
	let id = (u64::from(bus) << 8 | u64::from(dev) << 3 | u64::from(func)).into();
	let path = format!(
		"name:{}:{:02}.{}&vendor-id:{:04x}&device-id:{:04x}",
		bus,
		dev,
		func,
		h.vendor_id(),
		h.device_id(),
	)
	.into_boxed_str()
	.into();
	QueryResult { id, path }
}

fn n_to_bdf(n: u64) -> Option<(u8, u8, u8)> {
	let func = u8::try_from((n >> 0) & 0x07).unwrap();
	let dev = u8::try_from((n >> 3) & 0x1f).unwrap();
	let bus = u8::try_from((n >> 8) & 0xff).ok()?;
	Some((bus, dev, func))
}
