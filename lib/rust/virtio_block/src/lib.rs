#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

mod sector;

pub use sector::Sector;

use core::convert::TryInto;
use core::fmt;
use core::mem;
use core::ptr::NonNull;
use core::sync::atomic::{self, Ordering};
use endian::{u16le, u32le, u64le};
use memoffset::offset_of_tuple;
use virtio::{pci::CommonConfig, queue, PhysAddr, PhysRegion};

const SIZE_MAX: u32 = 1 << 1;
const SEG_MAX: u32 = 1 << 2;
const GEOMETRY: u32 = 1 << 4;
#[allow(dead_code)]
const RO: u32 = 1 << 5;
const BLK_SIZE: u32 = 1 << 6;
#[allow(dead_code)]
const FLUSH: u32 = 1 << 9;
const TOPOLOGY: u32 = 1 << 10;
#[allow(dead_code)]
const CONFIG_WCE: u32 = 1 << 11;
#[allow(dead_code)]
const DISCARD: u32 = 1 << 13;
#[allow(dead_code)]
const WRITE_ZEROES: u32 = 1 << 14;

#[allow(dead_code)]
const ANY_LAYOUT: u32 = 1 << 27;
#[allow(dead_code)]
const EVENT_IDX: u32 = 1 << 28;
#[allow(dead_code)]
const INDIRECT_DESC: u32 = 1 << 29;

/// A driver for a virtio block device.
pub struct BlockDevice<'a> {
	queue: queue::Queue<'a>,
	notify: virtio::pci::Notify<'a>,
	isr: &'a virtio::pci::ISR,
	request_header_status: NonNull<(RequestHeader, RequestStatus)>,
	request_header_status_phys: PhysAddr,
	/// The amount of sectors available
	_capacity: u64,
}

#[repr(C)]
struct Geometry {
	cylinders: u16,
	heads: u8,
	sectors: u8,
}

#[repr(C)]
struct Topology {
	physical_block_exp: u8,
	alignment_offset: u8,
	min_io_size: u16le,
	opt_io_size: u32le,
}

#[repr(C)]
struct Config {
	capacity: u64le,
	size_max: u32le,
	seg_max: u32le,
	geometry: Geometry,
	blk_size: u32le,
	topology: Topology,
	writeback: u8,
	_unused_0: [u8; 3],
	max_discard_sectors: u32le,
	max_discard_seg: u32le,
	discard_sector_alignment: u32le,
	max_write_zeroes_sectors: u32le,
	max_write_zeroes_seg: u32le,
	write_zeroes_may_unmap: u8,
	_unused_1: [u8; 3],
}

#[repr(C)]
struct RequestHeader {
	typ: u32le,
	reserved: u32le,
	sector: u64le,
}

impl RequestHeader {
	const READ: u32 = 0;
	const WRITE: u32 = 1;
}

#[repr(C)]
struct RequestStatus {
	status: u8,
}

/// PCI MSI-X configuration.
pub struct Msix {
	/// The MSI-X vector to use for queue interrupts.
	pub queue: Option<u16>,
}

impl<'a> BlockDevice<'a> {
	/// Setup a block device
	///
	/// This is meant to be used as a handler by the `virtio` crate.
	///
	/// # Safety
	///
	/// `dma_alloc` must return valid addresses.
	pub unsafe fn new<DmaError>(
		pci: &'a pci::Header0,
		map_bar: impl FnMut(u8) -> NonNull<()>,
		mut dma_alloc: impl FnMut(usize, usize) -> Result<(NonNull<()>, PhysAddr), DmaError>,
		msix: Msix,
	) -> Result<Self, SetupError<DmaError>> {
		let (request_header_status, request_header_status_phys) = dma_alloc(
			mem::size_of::<(RequestHeader, RequestStatus)>(),
			mem::align_of::<(RequestHeader, RequestStatus)>(),
		)
		.map_err(SetupError::DmaError)?;

		let dev = virtio::pci::Device::new(pci, map_bar).unwrap();

		dev.common.device_status.set(CommonConfig::STATUS_RESET);

		let features = SIZE_MAX | SEG_MAX | GEOMETRY | BLK_SIZE | TOPOLOGY;
		dev.common.device_feature_select.set(0.into());

		let features = u32le::from(features) & dev.common.device_feature.get();
		dev.common.device_feature.set(features);

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK,
		);
		// TODO check device status to ensure features were enabled correctly.

		let blk_cfg = unsafe { dev.device.cast::<Config>() };

		// Set up queue.
		let queue = queue::Queue::<'a>::new(dev.common, 0, 8, msix.queue, dma_alloc).map_err(
			|e| match e {
				queue::NewQueueError::DmaError(e) => SetupError::DmaError(e),
			},
		)?;

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK
				| CommonConfig::STATUS_DRIVER_OK,
		);

		Ok(Self {
			queue,
			notify: dev.notify,
			isr: dev.isr,
			request_header_status: request_header_status.cast(),
			request_header_status_phys,
			_capacity: blk_cfg.capacity.into(),
		})
	}

	/// Write out sectors
	///
	/// # Safety
	///
	/// The physical region must be valid.
	pub unsafe fn write<'s>(
		&'s mut self,
		data: PhysRegion,
		sector_start: u64,
		wait: impl FnMut(),
	) -> Result<(), WriteError> {
		unsafe {
			self.request_header_status.as_ptr().write((
				RequestHeader {
					typ: RequestHeader::WRITE.into(),
					reserved: 0.into(),
					sector: sector_start.into(),
				},
				RequestStatus { status: 111 },
			));
		}

		let data = [
			(
				self.request_header_status_phys
					+ u64::try_from(offset_of_tuple!((RequestHeader, RequestStatus), 0)).unwrap(),
				mem::size_of::<RequestHeader>().try_into().unwrap(),
				false,
			),
			(data.base, data.size, false),
			(
				self.request_header_status_phys
					+ u64::try_from(offset_of_tuple!((RequestHeader, RequestStatus), 1)).unwrap(),
				mem::size_of::<RequestStatus>().try_into().unwrap(),
				true,
			),
		];

		self.queue
			.send(data.iter().copied(), None, |_, _| ())
			.expect("Failed to send data");

		self.flush();

		self.queue.wait_for_used(|_, _| (), wait);

		Ok(())
	}

	/// Read in sectors
	///
	/// # Safety
	///
	/// The physical region must be valid.
	pub unsafe fn read<'s>(
		&'s mut self,
		data: PhysRegion,
		sector_start: u64,
		wait: impl FnMut(),
	) -> Result<(), WriteError> {
		unsafe {
			self.request_header_status.as_ptr().write((
				RequestHeader {
					typ: RequestHeader::READ.into(),
					reserved: 0.into(),
					sector: sector_start.into(),
				},
				RequestStatus { status: 111 },
			));
		}

		let data = [
			(
				self.request_header_status_phys
					+ u64::try_from(offset_of_tuple!((RequestHeader, RequestStatus), 0)).unwrap(),
				mem::size_of::<RequestHeader>().try_into().unwrap(),
				false,
			),
			(data.base, data.size, true),
			(
				self.request_header_status_phys
					+ u64::try_from(offset_of_tuple!((RequestHeader, RequestStatus), 1)).unwrap(),
				mem::size_of::<RequestStatus>().try_into().unwrap(),
				true,
			),
		];

		self.queue
			.send(data.iter().copied(), None, |_, _| ())
			.expect("Failed to send data");

		self.flush();

		self.queue.wait_for_used(|_, _| (), wait);

		Ok(())
	}

	pub fn flush(&self) {
		atomic::fence(Ordering::Release);
		self.notify.send(0);
	}

	#[inline]
	pub fn was_interrupted(&self) -> bool {
		self.isr.read().queue_update()
	}
}

impl Drop for BlockDevice<'_> {
	fn drop(&mut self) {
		todo!("ensure the device doesn't read/write memory after being dropped");
	}
}

#[derive(Debug)]
pub enum SetupError<DmaError> {
	DmaError(DmaError),
}

pub enum WriteError {}

impl fmt::Debug for WriteError {
	fn fmt(&self, _f: &mut fmt::Formatter) -> fmt::Result {
		/*
		f.write_str(match self {
		})
		*/
		Ok(())
	}
}
