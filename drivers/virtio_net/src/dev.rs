use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use virtio::{PhysAddr, PhysRegion};

struct DmaPacket {
	packet: NonNull<virtio_net::Packet>,
	packet_phys: PhysAddr,
	_marker: PhantomData<virtio_net::Packet>,
}

impl Deref for DmaPacket {
	type Target = virtio_net::Packet;

	fn deref(&self) -> &Self::Target {
		unsafe { self.packet.as_ref() }
	}
}

impl DerefMut for DmaPacket {
	fn deref_mut(&mut self) -> &mut Self::Target {
		unsafe { self.packet.as_mut() }
	}
}

struct DevInner<'d> {
	virtio: RefCell<virtio_net::Device<'d>>,
	rx_buffer: RefCell<DmaPacket>,
	tx_buffer: RefCell<DmaPacket>,
}

pub struct Dev<'d>(DevInner<'d>);

impl<'d> Dev<'d> {
	pub fn new(mut virtio: virtio_net::Device<'d>) -> Self {
		//let rx_tx_buffer = norostb_kernel::syscall::alloc_dma(None, 1514).unwrap();
		let (rx_tx_buffer, _) = norostb_kernel::syscall::alloc_dma(None, 2048 * 2).unwrap();
		let rx_tx_buffer_phys = norostb_kernel::syscall::physical_address(rx_tx_buffer).unwrap();
		let rx_tx_buffer_phys = PhysAddr::new(rx_tx_buffer_phys.try_into().unwrap());

		let mut rx_buffer = DmaPacket {
			packet: rx_tx_buffer.cast(),
			packet_phys: rx_tx_buffer_phys,
			_marker: PhantomData,
		};
		let tx_buffer = DmaPacket {
			packet: NonNull::new(
				rx_tx_buffer
					.cast::<u8>()
					.as_ptr()
					.wrapping_add(/*1514*/ 2048),
			)
			.unwrap()
			.cast(),
			packet_phys: rx_tx_buffer_phys + /*1514*/2048,
			_marker: PhantomData,
		};

		let rx_phys = rx_buffer.packet_phys;
		virtio.insert_buffer(&mut rx_buffer, rx_phys).unwrap();

		Self(DevInner {
			virtio: RefCell::new(virtio),
			rx_buffer: RefCell::new(rx_buffer),
			tx_buffer: RefCell::new(tx_buffer),
		})
	}
}

impl<'a, 'd: 'a> Device<'a> for Dev<'d> {
	type RxToken = DevRxToken<'a, 'd>;
	type TxToken = DevTxToken<'a, 'd>;

	fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
		unsafe {
			let n = self
				.0
				.virtio
				.borrow_mut()
				.receive(|_, phys| {
					assert_eq!(
						phys.base,
						self.0.rx_buffer.borrow().packet_phys,
						"rx packet region doesn't match"
					);
				})
				.unwrap();
			assert!(
				n < 2,
				"received more than one packet despite submitting only one buffer"
			);
			(n > 0).then(|| (DevRxToken(self), DevTxToken(self)))
		}
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

pub struct DevRxToken<'a, 'd: 'a>(&'a Dev<'d>);

impl<'a, 'd: 'a> RxToken for DevRxToken<'a, 'd> {
	fn consume<R, F>(self, _: Instant, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let mut rx_buffer = self.0 .0.rx_buffer.borrow_mut();
		let r = f(&mut rx_buffer.data);
		let phys = rx_buffer.packet_phys;
		self.0
			 .0
			.virtio
			.borrow_mut()
			.insert_buffer(&mut rx_buffer, phys)
			.unwrap();
		r
	}
}

pub struct DevTxToken<'a, 'd: 'a>(&'a Dev<'d>);

impl<'a, 'd: 'a> TxToken for DevTxToken<'a, 'd> {
	fn consume<R, F>(self, _: Instant, len: usize, f: F) -> smoltcp::Result<R>
	where
		F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
	{
		let mut tx_buffer = self.0 .0.tx_buffer.borrow_mut();
		let r = f(&mut tx_buffer.data[..len]);
		let phys = tx_buffer.packet_phys;
		unsafe {
			self.0
				 .0
				.virtio
				.borrow_mut()
				.send(
					&mut tx_buffer,
					PhysRegion {
						base: phys,
						size: virtio_net::Packet::size_with_data(len),
					},
					|| (),
				)
				.unwrap();
			r
		}
	}
}
