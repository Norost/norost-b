//! Based on https://docs.oasis-open.org/virtio/virtio/v1.1/cs01/virtio-v1.1-cs01.html#x1-2110003

#![no_std]
#![feature(alloc_layout_extra)]
#![feature(
	maybe_uninit_slice,
	maybe_uninit_write_slice,
	maybe_uninit_array_assume_init
)]

use core::alloc::Layout;
use core::convert::TryInto;
use core::fmt;
use core::mem;
use core::ptr::NonNull;
use endian::{u16le, u32le};
use virtio::pci::CommonConfig;
use virtio::queue;
use virtio::{PhysAddr, PhysRegion};

/// Device handles packets with partial checksum. This "checksum offload" is a common feature on
/// modern network cards.
#[allow(dead_code)]
const CSUM: u32 = 1 << 0;
/// Driver handles packets with partial checksum.
#[allow(dead_code)]
const GUEST_CSUM: u32 = 1 << 1;
/// Control channel offloads reconfiguration support.
#[allow(dead_code)]
const CTRL_GUEST_OFFLOADS: u32 = 1 << 2;
/// Device maximum MTU reporting is supported. If offered by the device, device advises driver
/// about the value of its maximum MTU. If negotiated, the driver uses mtu as the maximum MTU
/// value.
#[allow(dead_code)]
const MTU: u32 = 1 << 3;
/// Device has given MAC address.
const MAC: u32 = 1 << 5;
/// Driver can receive TSOv4.
#[allow(dead_code)]
const GUEST_TSO4: u32 = 1 << 7;
/// Driver can receive TSOv6.
#[allow(dead_code)]
const GUEST_TSO6: u32 = 1 << 8;
/// Driver can receive TSO with ECN.
#[allow(dead_code)]
const GUEST_ECN: u32 = 1 << 9;
/// Driver can receive UFO.
#[allow(dead_code)]
const GUEST_UFO: u32 = 1 << 10;
/// Device can receive TSOv4.
#[allow(dead_code)]
const HOST_TSO4: u32 = 1 << 11;
/// Device can receive TSOv6.
#[allow(dead_code)]
const HOST_TSO6: u32 = 1 << 12;
/// Device can receive TSO with ECN.
#[allow(dead_code)]
const HOST_ECN: u32 = 1 << 13;
/// Device can receive UFO.
#[allow(dead_code)]
const HOST_UFO: u32 = 1 << 14;
/// Driver can merge receive buffers.
#[allow(dead_code)]
const MRG_RXBUF: u32 = 1 << 15;
/// Configuration status field is available.
#[allow(dead_code)]
const STATUS: u32 = 1 << 16;
/// Control channel is available.
#[allow(dead_code)]
const CTRL_VQ: u32 = 1 << 17;
/// Control channel RX mode support.
#[allow(dead_code)]
const CTRL_RX: u32 = 1 << 18;
/// Control channel VLAN filtering.
#[allow(dead_code)]
const CTRL_VLAN: u32 = 1 << 19;
/// Driver can send gratuitous packets.
#[allow(dead_code)]
const GUEST_ANNOUNCE: u32 = 1 << 21;
/// Device supports multiqueue with automatic receive steering.
#[allow(dead_code)]
const MQ: u32 = 1 << 22;
/// Set MAC address through control channel.
#[allow(dead_code)]
const CTRL_MAC_ADDR: u32 = 1 << 23;
/// Device can process duplicated ACKs and report number of coalesced segments and duplicated ACKs.
#[allow(dead_code)]
const RSC_EXT: u32 = 1 << (61 - 32);
/// Device may act as a standby for a primary device with the same MAC address.
#[allow(dead_code)]
const STANDBY: u32 = 1 << (62 - 32);

#[repr(C)]
struct Config {
	mac: [u8; 6],
	status: u16le,
	max_virtqueue_pairs: u16le,
	mtu: u16le,
}

impl Config {
	#[allow(dead_code)]
	const STATUS_LINK_UP: u16 = 1 << 0;
	#[allow(dead_code)]
	const STATUS_ANNOUNCE: u16 = 1 << 1;
}

#[derive(Default)]
#[repr(C)]
struct PacketHeader {
	flags: u8,
	gso_type: u8,
	header_length: u16le,
	gso_size: u16le,
	csum_start: u16le,
	csum_offset: u16le,
	num_buffers: u16le,
}

impl fmt::Debug for PacketHeader {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_struct(stringify!(PacketHeader));
		f.field("flags", &self.flags);
		f.field("gso_type", &self.gso_type);
		let mut g = |n: &str, v: u16le| {
			f.field(n, &u16::from(v));
		};
		g("header_length", self.header_length);
		g("csum_start", self.csum_start);
		g("csum_offset", self.csum_offset);
		g("num_buffers", self.num_buffers);
		f.finish()
	}
}

impl PacketHeader {
	#[allow(dead_code)]
	const NEEDS_CSUM: u8 = 1 << 0;
	#[allow(dead_code)]
	const DATA_VALID: u8 = 1 << 1;
	#[allow(dead_code)]
	const RSC_INFO: u8 = 1 << 2;

	const GSO_NONE: u8 = 0;
	#[allow(dead_code)]
	const GSO_TCP4: u8 = 1;
	#[allow(dead_code)]
	const GSO_UDP: u8 = 3;
	#[allow(dead_code)]
	const GSO_TCP6: u8 = 4;
	#[allow(dead_code)]
	const GSO_ECN: u8 = 0x80;
}

#[repr(C)]
pub struct Packet {
	header: PacketHeader,
	pub data: [u8; Self::MAX_ETH_SIZE],
}

impl Packet {
	const MAX_ETH_SIZE: usize = 1514;
	/// There is no way to get the real size (_not_ stride), so this'll have to do.
	const MAX_SIZE: usize = mem::size_of::<PacketHeader>() + Self::MAX_ETH_SIZE;

	/// Calculate the total size of the packet with the given amount of data.
	///
	/// # Panics
	///
	/// `size` is larger than 1514.
	pub fn size_with_data(size: usize) -> u32 {
		assert!(size <= 1514, "size may not be larger than 1514");
		(mem::size_of::<PacketHeader>() + size).try_into().unwrap()
	}
}

impl Default for Packet {
	fn default() -> Self {
		Self {
			header: Default::default(),
			data: [0; Self::MAX_ETH_SIZE],
		}
	}
}

#[allow(dead_code)]
#[repr(C)]
struct NetworkControl {
	class: u8,
	command: u8,
	command_specific_data: [u8; 0],
	// ack: u8 after command_specific_data
}

pub struct Mac([u8; 6]);

impl AsRef<[u8; 6]> for Mac {
	fn as_ref(&self) -> &[u8; 6] {
		&self.0
	}
}

impl fmt::Debug for Mac {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "Mac({})", self)
	}
}

impl fmt::Display for Mac {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		for (i, &e) in self.0.iter().enumerate() {
			if i > 0 {
				f.write_str(":")?;
			}
			write!(f, "{:02x}", e)?;
		}
		Ok(())
	}
}

/// PCI MSI-X configuration.
pub struct Msix {
	/// The MSI-X vector to use for receive queue interrupts.
	pub receive_queue: Option<u16>,
	/// The MSI-X vector to use for transmit queue interrupts.
	pub transmit_queue: Option<u16>,
}

/// A driver for a virtio network (Ethernet) device.
pub struct Device<'a> {
	tx_queue: queue::Queue<'a>,
	rx_queue: queue::Queue<'a>,
	notify: virtio::pci::Notify<'a>,
	isr: &'a virtio::pci::ISR,
}

impl<'a> Device<'a> {
	/// Setup a network device
	pub unsafe fn new<DmaError>(
		pci: &'a pci::Header0,
		map_bar: impl FnMut(u8) -> NonNull<()>,
		mut dma_alloc: impl FnMut(usize, usize) -> Result<(NonNull<()>, PhysAddr), DmaError>,
		msix: Msix,
	) -> Result<(Self, Mac), SetupError<DmaError>> {
		let dev = virtio::pci::Device::new(pci, map_bar).unwrap();

		dev.common.device_status.set(CommonConfig::STATUS_RESET);
		dev.common
			.device_status
			.set(CommonConfig::STATUS_ACKNOWLEDGE);
		dev.common
			.device_status
			.set(CommonConfig::STATUS_ACKNOWLEDGE | CommonConfig::STATUS_DRIVER);

		let features = MAC;
		//let features = MAC | MRG_RXBUF;
		dev.common.device_feature_select.set(0.into());
		let features = u32le::from(features) & dev.common.device_feature.get();
		dev.common.driver_feature_select.set(0.into());
		dev.common.driver_feature.set(features);

		const VIRTIO_F_VERSION_1: u32 = 1 << (32 - 32);
		let features = VIRTIO_F_VERSION_1;
		dev.common.device_feature_select.set(1.into());
		let features = u32le::from(features) & dev.common.device_feature.get();
		assert_eq!(
			u32::from(features),
			VIRTIO_F_VERSION_1,
			"New virtio-net is unsupported"
		);
		dev.common.driver_feature_select.set(1.into());
		dev.common.driver_feature.set(features);

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK,
		);
		// TODO check device status to ensure features were enabled correctly.

		// Set up queues.
		let rx_queue =
			queue::Queue::<'a>::new(dev.common, 0, 8, msix.receive_queue, &mut dma_alloc).map_err(
				|e| match e {
					queue::NewQueueError::DmaError(e) => SetupError::DmaError(e),
				},
			)?;
		let tx_queue =
			queue::Queue::<'a>::new(dev.common, 1, 8, msix.transmit_queue, &mut dma_alloc)
				.map_err(|e| match e {
					queue::NewQueueError::DmaError(e) => SetupError::DmaError(e),
				})?;

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK
				| CommonConfig::STATUS_DRIVER_OK,
		);

		let mac = Mac(dev.device.cast::<Config>().mac);

		let s = Self {
			rx_queue,
			tx_queue,
			notify: dev.notify,
			isr: dev.isr,
		};
		Ok((s, mac))
	}

	/// Send an Ethernet packet
	///
	/// # Safety
	///
	/// `data` must remain valid for the duration of the transmission.
	/// `data_phys` must point to the same memory region as `data`.
	pub unsafe fn send<'s>(
		&'s mut self,
		mut data: NonNull<Packet>,
		data_phys: PhysRegion,
	) -> Result<(), SendError> {
		data.as_mut().header = PacketHeader {
			flags: 0,
			gso_type: PacketHeader::GSO_NONE,
			csum_start: 0.into(),
			csum_offset: 0.into(),
			gso_size: 0.into(),
			header_length: u16::try_from(mem::size_of::<PacketHeader>())
				.unwrap()
				.into(),
			num_buffers: 0.into(),
		};

		let data = [(data_phys.base, data_phys.size, false)];

		self.tx_queue
			.send(data.iter().copied(), None, |_, _| ())
			.expect("Failed to send data");

		self.notify.send(self.tx_queue.notify_offset());

		Ok(())
	}

	/// Collect buffers for sent packets.
	pub fn collect_sent(&mut self, mut f: impl FnMut(PhysRegion)) -> usize {
		self.tx_queue.collect_used(|_, r| f(r))
	}

	/// Receive a number of Ethernet packets, if any are available
	pub unsafe fn receive<'s>(
		&'s mut self,
		callback: impl FnMut(u16, PhysRegion),
	) -> Result<usize, ReceiveError> {
		Ok(self.rx_queue.collect_used(callback))
	}

	#[inline]
	pub fn was_interrupted(&self) -> bool {
		self.isr.read().queue_update()
	}

	/// Get the layout requirements of a single packet. Useful for allocation.
	pub fn packet_layout(&self) -> Layout {
		Layout::new::<Packet>()
			.extend_packed(Layout::new::<[u8; Packet::MAX_ETH_SIZE]>())
			.unwrap()
	}

	/// Insert a buffer for the device to write RX data to
	///
	/// # Safety
	///
	/// `data` and `data_phys` must be valid.
	pub unsafe fn insert_buffer<'s>(
		&'s mut self,
		mut data: NonNull<Packet>,
		data_phys: PhysAddr,
	) -> Result<(), Full> {
		data.as_mut().header = PacketHeader {
			flags: 12,
			gso_type: 34,
			csum_start: 5678.into(),
			csum_offset: 9012.into(),
			gso_size: 3456.into(),
			header_length: 7890.into(),
			num_buffers: 1234.into(),
		};

		let data = [(data_phys, Packet::MAX_SIZE.try_into().unwrap(), true)];

		self.rx_queue
			.send(data.iter().copied(), None, |_, _| ())
			.expect("Failed to send data");

		self.notify.send(self.rx_queue.notify_offset());

		Ok(())
	}
}

impl Drop for Device<'_> {
	fn drop(&mut self) {
		todo!("ensure the device doesn't read/write memory after being dropped");
	}
}

#[derive(Debug)]
pub enum SetupError<DmaError> {
	DmaError(DmaError),
}

pub enum SendError {}

impl fmt::Debug for SendError {
	fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
		/*
		f.write_str(match self {
		})
		*/
		Ok(())
	}
}

pub enum ReceiveError {}

impl fmt::Debug for ReceiveError {
	fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
		/*
		f.write_str(match self {
		})
		*/
		Ok(())
	}
}

#[derive(Debug)]
pub struct Full;
