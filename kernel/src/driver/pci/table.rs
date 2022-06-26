use crate::object_table::{Error, Object, Ticket};
use alloc::{boxed::Box, format, string::String, sync::Arc};
use core::sync::atomic::{AtomicU32, Ordering};

/// Table with all PCI devices.
pub struct PciTable;

impl Object for PciTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"" | b"/" => Ok(Arc::new(Query {
				index: AtomicU32::new(0),
			})),
			b"info" | b"info/" => Ok(Arc::new(Info {
				query: Query {
					index: AtomicU32::new(0),
				},
			})),
			_ => path_to_bdf(path)
				.and_then(|(bus, dev, func)| {
					let pci = super::PCI.lock();
					pci.as_ref()
						.unwrap()
						.get(bus, dev, func)
						.map(|d| pci_dev_object(d, bus, dev, func))
						.map(Ok)
				})
				.unwrap_or_else(|| Err(Error::DoesNotExist)),
		})
	}
}

struct Query {
	index: AtomicU32,
}

impl Query {
	fn next(&self) -> Option<((u16, u16), (u8, u8, u8))> {
		let pci = super::PCI.lock();
		let pci = pci.as_ref().unwrap();
		loop {
			let i = self.index.fetch_add(1, Ordering::Relaxed);
			if i >= 0x100 << 8 {
				break;
			}
			let (bus, dev, func) = n_to_bdf(i.into()).unwrap();
			if let Some(d) = pci.get(bus, dev, func) {
				return Some(((d.vendor_id(), d.device_id()), (bus, dev, func)));
			}
		}
		None
	}
}

impl Object for Query {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		// bb:dd.f
		Ticket::new_complete(if length < 2 + 1 + 2 + 1 + 1 {
			Err(Error::Unknown)
		} else {
			Ok(self.next().map_or([].into(), |(_, (b, d, f))| {
				bdf_to_string(b, d, f).into_bytes().into()
			}))
		})
	}
}

struct Info {
	query: Query,
}

impl Object for Info {
	fn read(&self, length: usize) -> Ticket<Box<[u8]>> {
		// bb:dd.f vvvv:dddd
		Ticket::new_complete(if length < (2 + 1 + 2 + 1 + 1) + 1 + (4 + 1 + 4) {
			Err(Error::Unknown)
		} else if let Some(((v, d), (bus, dev, func))) = self.query.next() {
			Ok(
				(bdf_to_string(bus, dev, func) + " " + &vendor_device_id_to_str(v, d))
					.into_bytes()
					.into(),
			)
		} else {
			Ok([].into())
		})
	}
}

fn bdf_to_string(bus: u8, dev: u8, func: u8) -> String {
	format!("{:02}:{:02}.{}", bus, dev, func)
}

fn pci_dev_object(_h: pci::Header, bus: u8, dev: u8, _func: u8) -> Arc<dyn Object> {
	Arc::new(super::PciDevice::new(bus, dev))
}

fn vendor_device_id_to_str(v: u16, d: u16) -> String {
	format!("{:04x}:{:04x}", v, d)
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
