//! Based on https://docs.oasis-open.org/virtio/virtio/v1.1/cs01/virtio-v1.1-cs01.html#x1-2110003

#![no_std]
#![feature(maybe_uninit_slice, maybe_uninit_write_slice)]

use core::alloc::Layout;
use core::convert::TryInto;
use core::fmt;
use core::mem::{self, MaybeUninit};
use core::ptr::NonNull;
use endian::{u16le, u32le, u64le};
use virtio::pci::CommonConfig;
use virtio::queue;

/// Device handles packets with partial checksum. This “checksum offload” is a common feature on
/// modern network cards.
const CSUM: u32 = 1 << 0;
/// Driver handles packets with partial checksum.
const GUEST_CSUM: u32 = 1 << 1;
/// Control channel offloads reconfiguration support.
const CTRL_GUEST_OFFLOADS: u32 = 1 << 2;
/// Device maximum MTU reporting is supported. If offered by the device, device advises driver
/// about the value of its maximum MTU. If negotiated, the driver uses mtu as the maximum MTU
/// value.
const MTU: u32 = 1 << 3;
/// Device has given MAC address.
const MAC: u32 = 1 << 5;
/// Driver can receive TSOv4.
const GUEST_TSO4: u32 = 1 << 7;
/// Driver can receive TSOv6.
const GUEST_TSO6: u32 = 1 << 8;
/// Driver can receive TSO with ECN.
const GUEST_ECN: u32 = 1 << 9;
/// Driver can receive UFO.
const GUEST_UFO: u32 = 1 << 10;
/// Device can receive TSOv4.
const HOST_TSO4: u32 = 1 << 11;
/// Device can receive TSOv6.
const HOST_TSO6: u32 = 1 << 12;
/// Device can receive TSO with ECN.
const HOST_ECN: u32 = 1 << 13;
/// Device can receive UFO.
const HOST_UFO: u32 = 1 << 14;
/// Driver can merge receive buffers.
const MRG_RXBUF: u32 = 1 << 15;
/// Configuration status field is available.
const STATUS: u32 = 1 << 16;
/// Control channel is available.
const CTRL_VQ: u32 = 1 << 17;
/// Control channel RX mode support.
const CTRL_RX: u32 = 1 << 18;
/// Control channel VLAN filtering.
const CTRL_VLAN: u32 = 1 << 19;
/// Driver can send gratuitous packets.
const GUEST_ANNOUNCE: u32 = 1 << 21;
/// Device supports multiqueue with automatic receive steering.
const MQ: u32 = 1 << 22;
/// Set MAC address through control channel.
const CTRL_MAC_ADDR: u32 = 1 << 23;
/// Device can process duplicated ACKs and report number of coalesced segments and duplicated ACKs.
const RSC_EXT: u32 = 1 << (61 - 32);
/// Device may act as a standby for a primary device with the same MAC address.
const STANDBY: u32 = 1 << (62 - 32);

#[repr(C)]
struct Config {
	mac: [u8; 6],
	status: u16le,
	max_virtqueue_pairs: u16le,
	mtu: u16le,
}

impl Config {
	const STATUS_LINK_UP: u16 = 1 << 0;
	const STATUS_ANNOUNCE: u16 = 1 << 1;
}

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
	const NEEDS_CSUM: u8 = 1 << 0;
	const DATA_VALID: u8 = 1 << 1;
	const RSC_INFO: u8 = 1 << 2;

	const GSO_NONE: u8 = 0;
	const GSO_TCP4: u8 = 1;
	const GSO_UDP: u8 = 3;
	const GSO_TCP6: u8 = 4;
	const GSO_ECN: u8 = 0x80;
}

#[allow(dead_code)]
#[repr(C)]
struct NetworkControl {
	class: u8,
	command: u8,
	command_specific_data: [u8; 0],
	// ack: u8 after command_specific_data
}

// Align packet to 2048 bytes to ensure it doesn't cross page boundaries.
// It also allows using just a single buffer/descriptor.
#[repr(align(2048))]
#[repr(C)]
struct Packet {
	header: PacketHeader,
	data: [MaybeUninit<u8>; Self::MAX_ETH_SIZE],
}

impl Packet {
	const MAX_ETH_SIZE: usize = 1514;
	/// There is no way to get the real size (_not_ stride), so this'll have to do.
	const MAX_SIZE: usize = mem::size_of::<PacketHeader>() + Self::MAX_ETH_SIZE;
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

/// A driver for a virtio network (Ethernet) device.
pub struct Device<'a, F>
where
	F: Fn(*const ()) -> usize,
{
	rx_packet: NonNull<Packet>,
	tx_queue: queue::Queue<'a>,
	rx_queue: queue::Queue<'a>,
	notify: virtio::pci::Notify<'a>,
	isr: &'a virtio::pci::ISR,
	get_physical_address: F,
}

impl<'a, F> Device<'a, F>
where
	F: Fn(*const ()) -> usize,
{
	/// Setup a network device
	pub fn new(
		pci: &'a pci::Header0,
		get_physical_address: F,
		map_bar: impl FnMut(u8) -> NonNull<()>,
		mut dma_alloc: impl FnMut(usize) -> Result<(NonNull<()>, usize), ()>,
	) -> Result<(Self, Mac), SetupError> {
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
			"Legacy virtio-net is unsupported"
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
			queue::Queue::<'a>::new(dev.common, 0, 8, None, &mut dma_alloc).expect("OOM");
		let tx_queue =
			queue::Queue::<'a>::new(dev.common, 1, 8, None, &mut dma_alloc).expect("OOM");

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK
				| CommonConfig::STATUS_DRIVER_OK,
		);

		let mac = Mac(unsafe { dev.device.cast::<Config>() }.mac);

		let rx_packet = dma_alloc(mem::size_of::<Packet>()).expect("OOM").0.cast();

		let mut s = Self {
			rx_packet,
			rx_queue,
			tx_queue,
			notify: dev.notify,
			isr: dev.isr,
			get_physical_address,
		};
		s.insert_buffer(s.rx_packet);
		Ok((s, mac))
	}

	/// Send an Ethernet packet
	///
	/// # Panics
	///
	/// The amount of data is larger than `MAX_ETH_SIZE`, i.e. 1514 bytes.
	pub fn send<'s>(&'s mut self, data: &'s [u8], wait: impl FnMut()) -> Result<(), SendError> {
		assert!(
			data.len() <= Packet::MAX_ETH_SIZE,
			"data len must be smaller or equal to MAX_ETH_SIZE"
		);
		let mut a = Packet {
			header: PacketHeader {
				flags: 0,
				gso_type: PacketHeader::GSO_NONE,
				csum_start: 0.into(),
				csum_offset: 0.into(),
				gso_size: 0.into(),
				header_length: u16::try_from(mem::size_of::<PacketHeader>())
					.unwrap()
					.into(),
				num_buffers: 0.into(),
			},
			data: [MaybeUninit::uninit(); Packet::MAX_ETH_SIZE],
		};
		MaybeUninit::write_slice(&mut a.data[..data.len()], &data);
		let phys = (self.get_physical_address)(&a as *const _ as *const _);

		let data = [(
			phys.try_into().unwrap(),
			(mem::size_of::<PacketHeader>() + data.len())
				.try_into()
				.unwrap(),
			false,
		)];

		self.tx_queue
			.send(data.iter().copied(), None, |_, _, _| ())
			.expect("Failed to send data");

		self.notify.send(self.tx_queue.notify_offset());

		self.tx_queue.wait_for_used(|_, _, _| (), wait);

		Ok(())
	}

	/// Receive an Ethernet packet, if any are available
	pub fn receive<'s>(&'s mut self, data: &'s mut [u8]) -> Result<bool, ReceiveError> {
		assert!(
			data.len() >= Packet::MAX_ETH_SIZE,
			"data len must be greater or equal to MAX_ETH_SIZE"
		);

		let n = self.rx_queue.collect_used(|_, _, _| ());
		assert!(n < 2, "received more than 1 packet at once");

		if n == 1 {
			let pkt = unsafe { self.rx_packet.as_ref() };
			// FIXME it seems QEMU isn't setting num_buffers properly if MRG_RXBUF is not
			// negotiated
			//assert_eq!(u16::from(pkt.header.num_buffers), 1, "expected only one buffer {:#?}", &pkt.header);
			// SAFETY: FIXME
			let pd = unsafe { MaybeUninit::slice_assume_init_ref(&pkt.data) };
			data[..Packet::MAX_ETH_SIZE].copy_from_slice(pd);
			self.insert_buffer(self.rx_packet);
			Ok(true)
		} else {
			Ok(false)
		}
	}

	#[inline]
	pub fn was_interrupted(&self) -> bool {
		self.isr.read().queue_update()
	}

	/// Insert a buffer for the device to write RX data to
	fn insert_buffer<'s>(&'s mut self, packet: NonNull<Packet>) {
		let phys = (self.get_physical_address)(packet.as_ptr() as *const _);

		let data = [(
			phys.try_into().unwrap(),
			Packet::MAX_SIZE.try_into().unwrap(),
			true,
		)];

		let pkt = unsafe {
			let mut packet = packet;
			packet.as_mut()
		};
		pkt.header = PacketHeader {
			flags: 12,
			gso_type: 34,
			csum_start: 5678.into(),
			csum_offset: 9012.into(),
			gso_size: 3456.into(),
			header_length: 7890.into(),
			num_buffers: 1234.into(),
		};

		self.rx_queue
			.send(data.iter().copied(), None, |_, _, _| ())
			.expect("Failed to send data");

		self.notify.send(self.rx_queue.notify_offset());
	}
}

impl<F> Drop for Device<'_, F>
where
	F: Fn(*const ()) -> usize,
{
	fn drop(&mut self) {
		todo!("ensure the device doesn't read/write memory after being dropped");
	}
}

pub enum SetupError {}

impl fmt::Debug for SetupError {
	fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
		//f.write_str(match self {
		//})
		Ok(())
	}
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
