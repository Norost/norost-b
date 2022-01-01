use kernel::syslog;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::Result;

pub struct Dev {
	rx_buffer: [u8; 1536],
	tx_buffer: [u8; 1536],
}

impl<'a> Dev {
	fn new() -> Dev {
		Dev {
			rx_buffer: [0; 1536],
			tx_buffer: [0; 1536],
		}
	}
}

impl<'a> Device<'a> for Dev {
	type RxToken = DevRxToken<'a>;
	type TxToken = DevTxToken<'a>;

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		Some((
			DevRxToken(&mut self.rx_buffer[..]),
			DevTxToken(&mut self.tx_buffer[..]),
		))
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		Some(DevTxToken(&mut self.tx_buffer[..]))
	}

	fn capabilities(&self) -> DeviceCapabilities {
		let mut caps = DeviceCapabilities::default();
		caps.max_transmission_unit = 1536;
		caps.max_burst_size = Some(1);
		caps.medium = Medium::Ethernet;
		caps
	}
}

pub struct DevRxToken<'a>(&'a mut [u8]);

impl<'a> RxToken for DevRxToken<'a> {
	fn consume<R, F>(mut self, _timestamp: Instant, f: F) -> Result<R>
	where
		F: FnOnce(&mut [u8]) -> Result<R>,
	{
		// TODO: receive packet into buffer
		let result = f(&mut self.0);
		syslog!("rx called");
		result
	}
}

pub struct DevTxToken<'a>(&'a mut [u8]);

impl<'a> TxToken for DevTxToken<'a> {
	fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> Result<R>
	where
		F: FnOnce(&mut [u8]) -> Result<R>,
	{
		let result = f(&mut self.0[..len]);
		syslog!("tx called {}", len);
		// TODO: send packet out
		result
	}
}
