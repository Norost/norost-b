use core::cell::RefCell;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;

pub struct Dev<'d, F>
where
	F: Fn(*const ()) -> usize + 'd,
{
	virtio: RefCell<virtio_net::Device<'d, F>>,
}

impl<'d, F> Dev<'d, F>
where
	F: Fn(*const ()) -> usize + 'd,
{
	pub fn new(virtio: virtio_net::Device<'d, F>) -> Self {
		Dev {
			virtio: RefCell::new(virtio),
		}
	}
}

impl<'a, 'd: 'a, F> Device<'a> for Dev<'d, F>
where
	F: Fn(*const ()) -> usize + 'd,
{
	type RxToken = DevRxToken;
	type TxToken = DevTxToken<'a, 'd, F>;

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		let mut data = [0; 1514];
		self.virtio
			.borrow_mut()
			.receive(&mut data)
			.unwrap()
			.then(|| (DevRxToken(data), DevTxToken(self)))
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		Some(DevTxToken(self))
	}

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = 1514;
		cap.max_burst_size = Some(1);
		cap.medium = Medium::Ethernet;
		cap
	}
}

pub struct DevRxToken([u8; 1514]);

impl RxToken for DevRxToken {
	fn consume<R, F>(mut self, _: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		f(&mut self.0)
	}
}

pub struct DevTxToken<'a, 'd: 'a, F>(&'a Dev<'d, F>)
where
	F: Fn(*const ()) -> usize + 'd;

impl<'a, 'd: 'a, PF> TxToken for DevTxToken<'a, 'd, PF>
where
	PF: Fn(*const ()) -> usize + 'd,
{
	fn consume<R, F>(self, _: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let mut data = [0; 1514];
		let r = f(&mut data[..len]);
		self.0
			.virtio
			.borrow_mut()
			.send(&mut data[..len], || ())
			.unwrap();
		r
	}
}
