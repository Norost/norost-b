#![no_std]

mod controlq;
mod cursorq;

extern crate alloc;

pub use controlq::{resource::create_2d::Format, Rect};

use controlq::{
	resource::{AttachBacking, Create2D, Flush, MemoryEntry},
	SetScanout, TransferToHost2D,
};
use core::{fmt, mem, num::NonZeroU32, ptr::NonNull};
use cursorq::{CursorPosition, MoveCursor, UpdateCursor};
use endian::{u32le, u64le};
use virtio::{
	pci::{CommonConfig, Notify},
	queue::{NewQueueError, Queue},
	PhysAddr, PhysMap,
};
use volatile::VolatileCell;

#[allow(dead_code)]
const FEATURE_VIRGL: u32 = 0x1;
const FEATURE_EDID: u32 = 0x2;

#[allow(dead_code)]
#[repr(C)]
struct Config {
	events_read: VolatileCell<u32le>,
	events_clear: VolatileCell<u32le>,
	num_scanouts: VolatileCell<u32le>,
	_reserved: u32le,
}

impl Config {
	#[allow(dead_code)]
	const EVENT_DISPLAY: u32 = 0x1;
}

#[derive(Clone, Copy)]
#[repr(C)]
struct ControlHeader {
	ty: u32le,
	flags: u32le,
	fence_id: u64le,
	context_id: u32le,
	_padding: u32le,
}

impl ControlHeader {
	const CMD_GET_DISPLAY_INFO: u32 = 0x100;
	const CMD_RESOURCE_CREATE_2D: u32 = 0x101;
	const CMD_RESOURCE_UNREF: u32 = 0x102;
	const CMD_SET_SCANOUT: u32 = 0x103;
	const CMD_RESOURCE_FLUSH: u32 = 0x104;
	const CMD_TRANSFER_TO_HOST_2D: u32 = 0x105;
	const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x106;
	const CMD_RESOURCE_DETACH_BACKING: u32 = 0x107;
	const CMD_GET_CAPSET_INFO: u32 = 0x108;
	const CMD_GET_CAPSET: u32 = 0x109;
	const CMD_GET_EDID: u32 = 0x110;

	const CMD_UPDATE_CURSOR: u32 = 0x300;
	const CMD_MOVE_CURSOR: u32 = 0x301;

	const RESP_OK_NODATA: u32 = 0x1100;
	const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
	const RESP_OK_CAPSET_INFO: u32 = 0x1102;
	const RESP_OK_CAPSET: u32 = 0x1103;
	const RESP_OK_EDID: u32 = 0x1104;

	const RESP_ERR_UNSPEC: u32 = 0x1200;
	const RESP_ERR_OUT_OF_MEMORY: u32 = 0x1201;
	const RESP_ERR_INVALID_SCANOUT_ID: u32 = 0x1202;
	const RESP_ERR_INVALID_RESOURCE_ID: u32 = 0x1203;
	const RESP_ERR_INVALID_CONTEXT_ID: u32 = 0x1204;
	const RESP_ERR_INVALID_PARAMETER: u32 = 0x1205;

	const FLAG_FENCE: u32 = 0x1;

	fn new(ty: u32, fence: Option<u64>) -> Self {
		Self {
			ty: ty.into(),
			flags: fence.map(|_| ControlHeader::FLAG_FENCE).unwrap_or(0).into(),
			fence_id: fence.unwrap_or(0).into(),
			context_id: 0.into(),
			_padding: 0.into(),
		}
	}
}

impl fmt::Debug for ControlHeader {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut d = f.debug_struct(stringify!(ControlHeader));

		let ty = match self.ty.into() {
			Self::CMD_GET_DISPLAY_INFO => "CMD_GET_DISPLAY_INFO",
			Self::CMD_RESOURCE_CREATE_2D => "CMD_RESOURCE_CREATE_2D",
			Self::CMD_RESOURCE_UNREF => "CMD_RESOURCE_UNREF",
			Self::CMD_SET_SCANOUT => "CMD_SET_SCANOUT",
			Self::CMD_RESOURCE_FLUSH => "CMD_RESOURCE_FLUSH",
			Self::CMD_TRANSFER_TO_HOST_2D => "CMD_TRANSFER_TO_HOST_2D",
			Self::CMD_RESOURCE_ATTACH_BACKING => "CMD_RESOURCE_ATTACH_BACKING",
			Self::CMD_RESOURCE_DETACH_BACKING => "CMD_RESOURCE_DETACH_BACKING",
			Self::CMD_GET_CAPSET_INFO => "CMD_GET_CAPSET_INFO",
			Self::CMD_GET_CAPSET => "CMD_GET_CAPSET",
			Self::CMD_GET_EDID => "CMD_GET_EDID",

			Self::CMD_UPDATE_CURSOR => "CMD_UPDATE_CURSOR",
			Self::CMD_MOVE_CURSOR => "CMD_MOVE_CURSOR",

			Self::RESP_OK_NODATA => "RESP_OK_NODATA",
			Self::RESP_OK_DISPLAY_INFO => "RESP_OK_DISPLAY_INFO",
			Self::RESP_OK_CAPSET_INFO => "RESP_OK_CAPSET_INFO",
			Self::RESP_OK_CAPSET => "RESP_OK_CAPSET",
			Self::RESP_OK_EDID => "RESP_OK_EDID",

			Self::RESP_ERR_UNSPEC => "RESP_ERR_UNSPEC",
			Self::RESP_ERR_OUT_OF_MEMORY => "RESP_ERR_OUT_OF_MEMORY",
			Self::RESP_ERR_INVALID_SCANOUT_ID => "RESP_ERR_INVALID_SCANOUT_ID",
			Self::RESP_ERR_INVALID_RESOURCE_ID => "RESP_ERR_INVALID_RESOURCE_ID",
			Self::RESP_ERR_INVALID_CONTEXT_ID => "RESP_ERR_INVALID_CONTEXT_ID",
			Self::RESP_ERR_INVALID_PARAMETER => "RESP_ERR_INVALID_PARAMETER",

			_ => "",
		};
		if ty == "" {
			d.field("type", &format_args!("0x{:x}", self.ty));
		} else {
			d.field("type", &format_args!("{}", ty));
		}

		let flags = u32::from(self.flags);
		if flags == Self::FLAG_FENCE {
			d.field("flags", &format_args!("FLAG_FENCE"));
		} else if flags & Self::FLAG_FENCE > 0 {
			d.field(
				"flags",
				&format_args!("FLAG_FENCE | 0x{:x}", flags & !Self::FLAG_FENCE),
			);
		} else {
			d.field("flags", &format_args!("0x{:x}", flags));
		}

		d.field("fence_id", &u64::from(self.fence_id));
		d.field("context_id", &u32::from(self.context_id));
		d.finish()
	}
}

/// Buffer for a scanout.
pub struct BackingStorage<'a> {
	storage: PhysMap<'a>,
}

#[repr(C)]
#[allow(dead_code)]
struct BackingStorageInner {
	attach: AttachBacking,
	mem_entries: [MemoryEntry; 0],
}

impl<'a> BackingStorage<'a> {
	/// Create a new [`BackingStorage`] with up to the given amount of memory entries.
	pub fn new(mut storage: PhysMap<'a>) -> Self {
		storage.write(&AttachBacking::new(0, 0, None));
		Self { storage }
	}

	/// Add an entry.
	///
	/// # Panics
	///
	/// The storage is full.
	#[track_caller]
	#[inline(always)]
	pub fn push(&mut self, map: &PhysMap<'a>) {
		self.try_push(map).expect("failed to add entry")
	}

	/// Try to add an entry. Returns an error if the storage is full.
	pub fn try_push(&mut self, map: &PhysMap<'a>) -> Result<(), virtio::phys::BufferTooSmall> {
		self.storage
			.try_split_at(self.total_size())?
			.1
			.write(&MemoryEntry::new(
				map.phys(),
				map.size().try_into().unwrap(),
			));
		self.attach_backing_mut().entities_count += 1;
		Ok(())
	}

	pub fn set_resource_id(&mut self, id: u32) {
		self.attach_backing_mut().resource_id = id.into();
	}

	fn attach_backing(&self) -> &AttachBacking {
		// SAFETY: we have written a valid AttachBacking in Self::new()
		unsafe { self.storage.virt().cast::<AttachBacking>().as_ref() }
	}

	fn attach_backing_mut(&mut self) -> &mut AttachBacking {
		// SAFETY: we have written a valid AttachBacking in Self::new()
		unsafe { self.storage.virt().cast::<AttachBacking>().as_mut() }
	}

	fn len(&self) -> usize {
		u32::from(self.attach_backing().entities_count)
			.try_into()
			.unwrap()
	}

	/// The total amount of valid data in the backing storage.
	fn total_size(&self) -> usize {
		mem::size_of::<AttachBacking>() + mem::size_of::<MemoryEntry>() * self.len()
	}
}

/// A handle to a resource
#[derive(Clone, Copy)]
pub struct Resource(NonZeroU32);

/// MSI-X interrupt vectors mappings per queue.
pub struct Msix {
	pub control: Option<u16>,
	pub cursor: Option<u16>,
}

pub struct Device<'a> {
	notify: Notify<'a>,
	controlq: Queue<'a>,
	cursorq: Queue<'a>,
}

impl<'a> Device<'a> {
	/// Setup a GPU device
	///
	/// This is meant to be used as a handler by the `virtio` crate.
	pub unsafe fn new<DmaError>(
		pci: &'a pci::Header0,
		map_bar: impl FnMut(u8) -> NonNull<()>,
		mut dma_alloc: impl FnMut(usize, usize) -> Result<(NonNull<()>, PhysAddr), DmaError>,
		msix: Msix,
	) -> Result<Self, SetupError<DmaError>> {
		let dev = virtio::pci::Device::new(pci, map_bar).unwrap();

		let features = FEATURE_EDID;
		dev.common.device_feature_select.set(0.into());

		let features = u32le::from(features) & dev.common.device_feature.get();
		dev.common.device_feature.set(features);

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK,
		);
		// TODO check device status to ensure features were enabled correctly.

		let map_err = |e| match e {
			NewQueueError::DmaError(e) => SetupError::DmaError(e),
		};
		let controlq =
			Queue::<'a>::new(dev.common, 0, 8, msix.control, &mut dma_alloc).map_err(map_err)?;
		let cursorq =
			Queue::<'a>::new(dev.common, 1, 8, msix.cursor, &mut dma_alloc).map_err(map_err)?;

		dev.common.device_status.set(
			CommonConfig::STATUS_ACKNOWLEDGE
				| CommonConfig::STATUS_DRIVER
				| CommonConfig::STATUS_FEATURES_OK
				| CommonConfig::STATUS_DRIVER_OK,
		);

		Ok(Self {
			controlq,
			cursorq,
			notify: dev.notify,
		})
	}

	pub unsafe fn init_scanout(
		&mut self,
		format: Format,
		rect: Rect,
		backend: BackingStorage<'static>,
		buffer: &mut PhysMap,
	) -> Result<Resource, InitScanoutError> {
		let res_id = 1;
		let scan_id = 0;

		self.create_resource_2d(
			NonZeroU32::new(res_id).unwrap(),
			rect,
			format,
			backend,
			buffer,
		);

		// Attach scanout
		self.control_request(buffer, SetScanout::new(scan_id, res_id, rect, Some(0)))
			.unwrap();

		Ok(Resource(NonZeroU32::new(res_id).unwrap()))
	}

	pub unsafe fn init_cursor(
		&mut self,
		x: u32,
		y: u32,
		format: Format,
		backend: BackingStorage<'static>,
		buffer: &mut PhysMap,
	) -> Result<Resource, InitCursorError> {
		let res_id = 2;
		let scan_id = 0;

		let rect = Rect::new(0, 0, 64, 64);
		self.create_resource_2d(
			NonZeroU32::new(res_id).unwrap(),
			rect,
			format,
			backend,
			buffer,
		);

		let pos = CursorPosition::new(scan_id, x, y);
		self.cursor_request(buffer, UpdateCursor::new(pos, res_id, 0, 0, Some(0)))
			.unwrap();

		Ok(Resource(NonZeroU32::new(res_id).unwrap()))
	}

	pub fn update_cursor(
		&mut self,
		resource: Resource,
		hot_x: u32,
		hot_y: u32,
		buffer: &mut PhysMap,
	) -> Result<Resource, UpdateCursorError> {
		let res_id = resource.0.get();
		let scan_id = 0;
		let pos = CursorPosition::new(scan_id, 0, 0);
		self.cursor_request(
			buffer,
			UpdateCursor::new(pos, res_id, hot_x, hot_y, Some(0)),
		)
		.unwrap();
		Ok(Resource(NonZeroU32::new(res_id).unwrap()))
	}

	pub fn move_cursor(
		&mut self,
		x: u32,
		y: u32,
		buffer: &mut PhysMap,
	) -> Result<(), MoveCursorError> {
		let scan_id = 0;
		let pos = CursorPosition::new(scan_id, x, y);
		self.cursor_request(buffer, MoveCursor::new(pos, Some(0)))
			.unwrap();
		Ok(())
	}

	pub fn draw(
		&mut self,
		resource: Resource,
		rect: Rect,
		buffer: &mut PhysMap,
	) -> Result<(), DrawError> {
		let res_id = resource.0.get();
		self.control_request(buffer, TransferToHost2D::new(res_id, 0, rect, Some(0)))
			.unwrap();
		self.control_request(
			buffer,
			Flush::new(res_id.try_into().unwrap(), rect, Some(0)),
		)
		.unwrap();
		Ok(())
	}

	/// # Panics
	///
	/// `buffer` is smaller than [`ControlHeader`].
	fn create_resource_2d(
		&mut self,
		id: NonZeroU32,
		rect: Rect,
		format: Format,
		mut backend: BackingStorage,
		buffer: &mut PhysMap,
	) {
		backend.set_resource_id(id.get());
		self.control_request(
			buffer,
			Create2D::new(id.get(), format, rect.width(), rect.height(), Some(0)),
		)
		.unwrap();
		self.control_request_raw(
			buffer,
			backend.storage.phys(),
			backend.total_size().try_into().unwrap(),
		)
		.unwrap();
	}

	/// Send a request to the control queue.
	fn control_request<T: Copy>(&mut self, buf: &mut PhysMap, data: T) -> Result<(), ()> {
		Self::request(&mut self.controlq, &self.notify, 0, buf, data)
	}

	/// Send a request to the control queue.
	fn cursor_request<T: Copy>(&mut self, buf: &mut PhysMap, data: T) -> Result<(), ()> {
		Self::request(&mut self.cursorq, &self.notify, 1, buf, data)
	}

	/// Send a request with raw data to the control queue.
	fn control_request_raw(
		&mut self,
		buf: &mut PhysMap,
		data: PhysAddr,
		len: u32,
	) -> Result<(), ()> {
		Self::request_raw(&mut self.controlq, &self.notify, 0, buf, data, len)
	}

	/// Send a request to a queue.
	fn request<T: Copy>(
		queue: &mut Queue<'_>,
		notify: &Notify<'_>,
		queue_id: u16,
		buf: &mut PhysMap,
		data: T,
	) -> Result<(), ()> {
		let (mut resp, mut data_buf) = buf.split_at(mem::size_of::<ControlHeader>());
		data_buf.write(&data);
		Self::request_raw(
			queue,
			notify,
			queue_id,
			&mut resp,
			data_buf.phys(),
			mem::size_of::<T>().try_into().unwrap(),
		)
	}

	/// Send a request with raw data to a queue.
	fn request_raw(
		queue: &mut Queue<'_>,
		notify: &Notify<'_>,
		queue_id: u16,
		resp: &mut PhysMap,
		data: PhysAddr,
		len: u32,
	) -> Result<(), ()> {
		resp.write(&ControlHeader::new(0, None));

		let data = [
			(data, len, false),
			(
				resp.phys(),
				mem::size_of::<ControlHeader>().try_into().unwrap(),
				true,
			),
		];
		queue
			.send(data.iter().copied(), None, |_, _| ())
			.expect("failed to send data");
		notify.send(queue_id);
		queue.wait_for_used(|_, _| (), || ());

		Ok(())
	}
}

#[derive(Debug)]
pub enum SetupError<DmaError> {
	DmaError(DmaError),
}

#[derive(Debug)]
pub enum InitScanoutError {}

#[derive(Debug)]
pub enum InitCursorError {}

#[derive(Debug)]
pub enum UpdateCursorError {}

#[derive(Debug)]
pub enum MoveCursorError {}

#[derive(Debug)]
pub enum DrawError {}
