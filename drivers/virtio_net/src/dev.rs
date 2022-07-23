use core::{
	cell::RefCell,
	mem::{self, ManuallyDrop},
	num::NonZeroUsize,
	ptr::NonNull,
};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use virtio::{PhysAddr, PhysRegion};
use virtio_net::Packet;

const MAX_RX_PKT: usize = 8;
const MAX_TX_PKT: usize = 8;

struct DevInner<'d> {
	virtio: virtio_net::Device<'d>,
	/// First half are for RX packets, second half for TX.
	dma_virt: NonNull<Packet>,
	/// First half are for RX packets, second half for TX.
	dma_phys: u64,
	dma_size: NonZeroUsize,
	/// Bitmap of available RX packets.
	rx_avail_map: u64,
	/// Bitmap of available TX packets.
	tx_avail_map: u64,
}

impl<'d> DevInner<'d> {
	fn get(&mut self, i: usize) -> (NonNull<Packet>, PhysAddr) {
		assert!(i < MAX_RX_PKT + MAX_TX_PKT);
		unsafe {
			let phys = PhysAddr::new(self.dma_phys + (i * mem::size_of::<Packet>()) as u64);
			let virt = NonNull::new_unchecked(self.dma_virt.as_ptr().add(i));
			(virt, phys)
		}
	}

	fn pop_rx(&mut self) -> Option<usize> {
		pop_bit(&mut self.rx_avail_map)
	}

	fn pop_tx(&mut self) -> Option<usize> {
		pop_bit(&mut self.tx_avail_map)
	}
}

pub struct Dev<'d>(RefCell<DevInner<'d>>);

impl<'d> Dev<'d> {
	pub fn new(mut virtio: virtio_net::Device<'d>) -> Self {
		let (dma_virt, dma_phys, dma_size) = driver_utils::dma::alloc_dma(
			(mem::size_of::<Packet>() * (MAX_TX_PKT + MAX_RX_PKT))
				.try_into()
				.unwrap(),
		)
		.unwrap();
		let dma_virt = dma_virt.cast();

		let mut s = Self(
			DevInner {
				virtio,
				dma_virt,
				dma_phys,
				dma_size,
				rx_avail_map: 0x00ff,
				tx_avail_map: 0xff00,
			}
			.into(),
		);

		// Give first half to virtio device
		unsafe {
			for i in 0..MAX_RX_PKT {
				let (virt, phys) = s.0.get_mut().get(i);
				s.0.get_mut().virtio.insert_buffer(virt, phys).unwrap();
			}
			s.0.get_mut().rx_avail_map = 0;
		}

		s
	}

	/// Collect received packets & finished transactions.
	///
	/// Returns `true` if any RX packets are available.
	pub fn process(&mut self) -> bool {
		unsafe {
			let mut s = self.0.get_mut();
			let dma_phys = s.dma_phys;

			let calc_i =
				|phys: PhysAddr| (u64::from(phys.0) - dma_phys) / mem::size_of::<Packet>() as u64;

			let mut map = s.tx_avail_map;
			s.virtio.collect_sent(|_, r| {
				let i = calc_i(r.base);
				debug_assert_eq!(map & 1 << i, 0);
				map |= 1 << i;
			});
			s.tx_avail_map = map;

			let mut map = s.rx_avail_map;
			s.virtio
				.receive(|_, phys| {
					let i = calc_i(phys.base);
					debug_assert_eq!(map & 1 << i, 0);
					map |= 1 << i;
				})
				.unwrap();
			s.rx_avail_map = map;
			map != 0
		}
	}
}

fn pop_bit(m: &mut u64) -> Option<usize> {
	(*m != 0).then(|| {
		let i = m.trailing_zeros();
		*m &= !(1 << i);
		i as _
	})
}

impl<'a, 'd: 'a> Device<'a> for Dev<'d> {
	type RxToken = DevRxToken<'a, 'd>;
	type TxToken = DevTxToken<'a, 'd>;

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		self.0.get_mut().pop_rx().and_then(|index| {
			let tx = self.0.get_mut().pop_tx();
			let rx = DevRxToken {
				dev: &self.0,
				index,
			};
			tx.map(|index| {
				let tx = DevTxToken {
					dev: &self.0,
					index,
				};
				(rx, tx)
			})
		})
	}

	fn transmit(&'a mut self) -> Option<Self::TxToken> {
		self.0.get_mut().pop_tx().map(|index| DevTxToken {
			dev: &self.0,
			index,
		})
	}

	fn capabilities(&self) -> DeviceCapabilities {
		let mut cap = DeviceCapabilities::default();
		cap.max_transmission_unit = 1514;
		cap.max_burst_size = Some(MAX_RX_PKT.min(MAX_TX_PKT));
		cap.medium = Medium::Ethernet;
		cap
	}
}

pub struct DevRxToken<'a, 'd: 'a> {
	dev: &'a RefCell<DevInner<'d>>,
	index: usize,
}

impl<'a, 'd: 'a> RxToken for DevRxToken<'a, 'd> {
	fn consume<R, F>(self, _: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		unsafe {
			let (mut virt, phys) = self.dev.borrow_mut().get(self.index);
			let r = f(&mut virt.as_mut().data);
			ManuallyDrop::new(self)
				.dev
				.borrow_mut()
				.virtio
				.insert_buffer(virt, phys)
				.unwrap();
			r
		}
	}
}

impl Drop for DevRxToken<'_, '_> {
	fn drop(&mut self) {
		self.dev.borrow_mut().rx_avail_map |= 1 << self.index;
	}
}

pub struct DevTxToken<'a, 'd: 'a> {
	dev: &'a RefCell<DevInner<'d>>,
	index: usize,
}

impl<'a, 'd: 'a> TxToken for DevTxToken<'a, 'd> {
	fn consume<R, F>(self, _: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		unsafe {
			let (mut virt, phys) = self.dev.borrow_mut().get(self.index);
			let r = f(&mut virt.as_mut().data[..len]);
			ManuallyDrop::new(self)
				.dev
				.borrow_mut()
				.virtio
				.send(
					virt,
					PhysRegion {
						base: phys,
						size: Packet::size_with_data(len),
					},
				)
				.unwrap();
			r
		}
	}
}

impl Drop for DevTxToken<'_, '_> {
	fn drop(&mut self) {
		self.dev.borrow_mut().tx_avail_map |= 1 << self.index;
	}
}
