use core::convert::TryFrom;
use core::fmt;
use core::marker::PhantomData;
use core::ptr::NonNull;
use endian::{u16le, u32le, u64le};
use volatile::VolatileCell;

/// An identifier for a device type
#[derive(Clone, Copy, Hash, PartialOrd, Ord, Eq, PartialEq)]
pub struct DeviceType(u32);

impl DeviceType {
	/// Create a new device type identifier.
	#[inline(always)]
	pub fn new(vendor: u16, device: u16) -> Self {
		Self((u32::from(vendor) << 16) | u32::from(device))
	}

	/// Get the vendor of this device.
	#[inline(always)]
	pub fn vendor(&self) -> u16 {
		(self.0 >> 16) as u16
	}

	/// Get the type of device.
	#[inline(always)]
	pub fn device(&self) -> u16 {
		(self.0 & 0xffff) as u16
	}
}

impl fmt::Debug for DeviceType {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(DeviceType))
			.field("vendor", &self.vendor())
			.field("device", &self.device())
			.finish()
	}
}

#[repr(C)]
struct Capability {
	_next_cap: pci::Capability,
	capability_length: VolatileCell<u8>,
	config_type: VolatileCell<u8>,
	base_address: VolatileCell<u8>,
	padding: [u8; 3],
	offset: VolatileCell<u32le>,
	length: VolatileCell<u32le>,
	more_stuff: VolatileCell<u32le>, // TODO
}

impl Capability {
	pub const COMMON_CONFIGURATION: u8 = 1;
	pub const NOTIFY_CONFIGURATION: u8 = 2;
	pub const ISR_CONFIGURATION: u8 = 3;
	pub const DEVICE_CONFIGURATION: u8 = 4;
	#[allow(dead_code)]
	pub const PCI_CONFIGURATION: u8 = 5;
}

#[repr(C)]
pub struct CommonConfig {
	pub device_feature_select: VolatileCell<u32le>,
	pub device_feature: VolatileCell<u32le>,
	pub driver_feature_select: VolatileCell<u32le>,
	pub driver_feature: VolatileCell<u32le>,

	pub msix_config: VolatileCell<u16le>,
	pub queue_count: VolatileCell<u16le>,

	pub device_status: VolatileCell<u8>,
	pub config_generation: VolatileCell<u8>,

	pub queue_select: VolatileCell<u16le>,
	pub queue_size: VolatileCell<u16le>,
	pub queue_msix_vector: VolatileCell<u16le>,
	pub queue_enable: VolatileCell<u16le>,
	pub queue_notify_off: VolatileCell<u16le>,
	pub queue_descriptors: VolatileCell<u64le>,
	pub queue_driver: VolatileCell<u64le>,
	pub queue_device: VolatileCell<u64le>,
}

impl CommonConfig {
	pub const STATUS_RESET: u8 = 0x0;
	pub const STATUS_ACKNOWLEDGE: u8 = 0x1;
	pub const STATUS_DRIVER: u8 = 0x2;
	pub const STATUS_DRIVER_OK: u8 = 0x4;
	pub const STATUS_FEATURES_OK: u8 = 0x8;
	pub const STATUS_DEVICE_NEED_RESET: u8 = 0x40;
	pub const STATUS_FAILED: u8 = 0x80;
}

#[repr(C)]
pub struct ISR {
	status: VolatileCell<ISRStatus>,
}

impl ISR {
	/// Read the ISR status, clearing it.
	pub fn read(&self) -> ISRStatus {
		self.status.get()
	}
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ISRStatus(u8);

impl ISRStatus {
	const QUEUE_INTERRUPT: u8 = 0x1;
	const CONFIGURATION_INTERRUPT: u8 = 0x2;

	/// Whether an interrupt for a queue update was issued.
	#[inline]
	pub fn queue_update(&self) -> bool {
		self.0 & Self::QUEUE_INTERRUPT > 0
	}

	/// Whether an interrupt for a configuration update was issued.
	#[inline]
	pub fn configuration_update(&self) -> bool {
		self.0 & Self::CONFIGURATION_INTERRUPT > 0
	}
}

/// Device specific configuration struct.
///
/// The fields of this struct are empty as there are no common fields.
#[repr(C)]
pub struct DeviceConfig(());

impl DeviceConfig {
	pub unsafe fn cast<'a, T>(&'a self) -> &'a T {
		&*(self as *const _ as *const _)
	}
}

pub struct Notify<'a> {
	address: NonNull<VolatileCell<u16le>>,
	multiplier: u32,
	_marker: PhantomData<&'a VolatileCell<u16le>>,
}

impl Notify<'_> {
	pub fn send(&self, offset: u16) {
		unsafe {
			let offt = usize::try_from(self.multiplier / 2).unwrap() * usize::from(offset);
			(&*self.address.as_ptr().add(offt)).set(0.into())
		};
	}
}

pub struct Device<'a> {
	pub common: &'a CommonConfig,
	pub device: &'a DeviceConfig,
	pub notify: Notify<'a>,
	pub isr: &'a ISR,
}

impl<'a> Device<'a> {
	/// Setup a new virtio device on a PCI bus.
	pub fn new(
		header: &'a pci::Header0,
		mut map_bar: impl FnMut(u8) -> NonNull<()>,
	) -> Result<Device<'a>, ()> {
		let mut common = None;
		let mut notify = None;
		let mut isr = None;
		let mut device = None;

		for cap_raw in header.capabilities() {
			if cap_raw.id() == 0x9 {
				let cap = unsafe { cap_raw.data::<Capability>() };
				match cap.config_type.get() {
					Capability::COMMON_CONFIGURATION => {
						common.is_none().then(|| common = Some(cap));
					}
					Capability::NOTIFY_CONFIGURATION => {
						if notify.is_none() {
							let mul = cap.more_stuff.get().into();
							notify = Some((cap, mul));
						}
					}
					Capability::ISR_CONFIGURATION => {
						isr.is_none().then(|| isr = Some(cap));
					}
					Capability::DEVICE_CONFIGURATION => {
						device.is_none().then(|| device = Some(cap));
					}
					// There may exist other config types. We should ignore any we don't know.
					_ => (),
				}
			}
		}

		let common = common.unwrap();
		let notify = notify.unwrap();
		let isr = isr.unwrap();
		let device = device.unwrap();

		let mut mapped_bars = [None; 6];

		let mut mb = |cap: &Capability| {
			let bar = cap.base_address.get();
			let offt = usize::try_from(u32::from(cap.offset.get())).unwrap();
			let addr = mapped_bars[usize::from(bar)].unwrap_or_else(|| {
				let addr = map_bar(bar);
				mapped_bars[usize::from(bar)] = Some(addr);
				addr
			});
			NonNull::new(addr.as_ptr().cast::<u8>().wrapping_add(offt)).unwrap()
		};

		unsafe {
			let common = mb(common).cast().as_ref();
			let (notify, mul) = (mb(notify.0), notify.1);
			let isr = mb(isr).cast().as_ref();
			let device = mb(device).cast().as_ref();

			let notify = Notify {
				address: notify.cast(),
				multiplier: mul,
				_marker: PhantomData,
			};

			Ok(Device {
				common,
				device,
				notify,
				isr,
			})
		}
	}
}
