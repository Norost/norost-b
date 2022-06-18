//! Library for iterating and interacting with Pci and PCIe devices.
//!
//! ## References
//!
//! [Pci on OSDev wiki][osdev pci]
//!
//! [osdev pci]: https://wiki.osdev.org/Pci

#![no_std]
#![feature(ptr_metadata)]

use core::cell::Cell;
use core::convert::TryInto;
use core::fmt;
use core::marker::PhantomData;
use core::num::NonZeroU32;
use core::ptr::NonNull;
use endian::{u16le, u32le};
use volatile::VolatileCell;

pub const BAR_IO_SPACE: u32 = 1;
pub const BAR_TYPE_MASK: u32 = 0x6;

/// Representation of a base address (BAR).
///
/// I/O bar layout:
///
/// ```
/// +------------------------+----------+----------+
/// | 31 - 2                 | 1        | 0        |
/// +------------------------+----------+----------+
/// | 4 byte aligned address | reserved | always 0 |
/// +------------------------+----------+----------+
/// ```
///
/// MMIO bar layout:
///
/// ```
/// +-------------------------+--------------+-------+----------+
/// | 31 - 4                  | 3            | 1 - 2 | 0        |
/// +-------------------------+--------------+-------+----------+
/// | 16 byte aligned address | prefetchable | type  | always 1 |
/// +-------------------------+--------------+-------+----------+
/// ```
#[repr(transparent)]
pub struct BaseAddress(VolatileCell<u32le>);

impl BaseAddress {
	/// Check if a BAR value indicates an MMIO BAR.
	pub fn is_mmio(value: u32) -> bool {
		value & 1 == 0
	}

	/// Check if a BAR value indicates an I/O BAR.
	pub fn is_io(value: u32) -> bool {
		value & 1 == 1
	}

	/// Check if a BAR value indicates a 32 bit BAR.
	pub fn is_64bit(value: u32) -> bool {
		value & 0x6 == 0x4
	}

	/// Check if a BAR value indicates a 64 bit BAR.
	pub fn is_32bit(value: u32) -> bool {
		value & 0x6 == 0x0
	}

	/// Return the physical address the BAR(s) point(s) to. This may be a 64 bit address
	pub fn address(lower: u32, upper: impl FnOnce() -> Option<u32>) -> Option<u64> {
		if Self::is_64bit(lower) {
			Some(u64::from(lower & !0xf) | u64::from(upper()?) << 32)
		} else if Self::is_mmio(lower) {
			Some(u64::from(lower & !0xf))
		} else {
			None
		}
	}

	/// If set, reads won't have any side effects. This is useful to make better use of caching.
	pub fn is_prefetchable(value: u32) -> bool {
		value & 0x8 > 0
	}

	/// Get the full address one or two BARs point to. This may be 64-bit.
	///
	/// Returns `None` if the BAR is invalid.
	pub fn full_base_address(bars: &[Self], index: usize) -> Option<ParsedBaseAddress> {
		let low = bars.get(index)?.0.get().into();
		if BaseAddress::is_io(low) {
			Some(ParsedBaseAddress::IO32 {
				address: low & !0x3,
			})
		} else if BaseAddress::is_32bit(low) {
			Some(ParsedBaseAddress::MMIO32 {
				address: low & !0xf,
				prefetchable: BaseAddress::is_prefetchable(low),
			})
		} else if BaseAddress::is_64bit(low) {
			Some(ParsedBaseAddress::MMIO64 {
				address: u64::from(low & !0xf)
					| u64::from(u32::from(bars.get(index + 1)?.0.get())) << 32,
				prefetchable: BaseAddress::is_prefetchable(low),
			})
		} else {
			None
		}
	}

	/// Return the size of the memory area a BAR points to.
	///
	/// This dirties the register, so the original value must be restored afterwards (if any).
	///
	/// If the returned size is None, the original value does not need to be restored.
	///
	/// # Returns
	///
	/// The size as well as the original value. The size is None if the masked value is 0.
	#[must_use = "this call dirties the register"]
	pub fn size(&self) -> (Option<NonZeroU32>, u32) {
		let og = self.get();
		let mask = match Self::is_mmio(og) {
			true => !0xf,
			false => !0x3,
		};
		self.set(u32::MAX);
		let masked = self.get() & mask;
		(
			(masked != 0).then(|| NonZeroU32::new(!masked + 1).unwrap()),
			og,
		)
	}

	/// Return the raw value.
	#[must_use = "volatile loads cannot be optimized out"]
	pub fn get(&self) -> u32 {
		self.0.get().into()
	}

	/// Set the raw value.
	pub fn set(&self, value: u32) {
		self.0.set(value.into());
	}
}

impl fmt::Debug for BaseAddress {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "0x{:08x}", self.get())
	}
}

/// A BAR in a more friendly format.
pub enum ParsedBaseAddress {
	IO32 { address: u32 },
	MMIO32 { address: u32, prefetchable: bool },
	MMIO64 { address: u64, prefetchable: bool },
}

impl ParsedBaseAddress {
	#[inline]
	pub fn try_as_mmio(&self) -> Option<u64> {
		match self {
			Self::IO32 { .. } => None,
			Self::MMIO32 { address, .. } => Some(u64::from(*address)),
			Self::MMIO64 { address, .. } => Some(*address),
		}
	}
}

/// Common header fields.
#[repr(C)]
pub struct HeaderCommon {
	vendor_id: VolatileCell<u16le>,
	device_id: VolatileCell<u16le>,

	pub command: VolatileCell<u16le>,
	pub status: VolatileCell<u16le>,

	revision_id: VolatileCell<u8>,
	prog_if: VolatileCell<u8>,
	subclass: VolatileCell<u8>,
	class_code: VolatileCell<u8>,

	cache_line_size: VolatileCell<u8>,
	latency_timer: VolatileCell<u8>,
	header_type: VolatileCell<u8>,
	bist: VolatileCell<u8>,
}

macro_rules! get_volatile {
	($f:ident -> $t:ty) => {
		pub fn $f(&self) -> $t {
			self.$f.get().into()
		}
	};
}

macro_rules! set_volatile {
	($fn:ident : $f:ident <- $t:ty) => {
		pub fn $fn(&self, value: $t) {
			self.$f.set(value.into())
		}
	};
}

impl HeaderCommon {
	/// Flag used to enable MMIO
	pub const COMMAND_MMIO_MASK: u16 = 0x2;
	/// Flag used to toggle bus mastering.
	pub const COMMAND_BUS_MASTER_MASK: u16 = 0x4;
	/// Flag used to disable interrupts.
	pub const COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;

	get_volatile!(vendor_id -> u16);
	get_volatile!(device_id -> u16);
	get_volatile!(command -> u16);
	get_volatile!(status -> u16);
	get_volatile!(revision_id -> u8);
	get_volatile!(prog_if -> u8);
	get_volatile!(subclass -> u8);
	get_volatile!(class_code -> u8);
	get_volatile!(cache_line_size -> u8);
	get_volatile!(latency_timer -> u8);
	get_volatile!(header_type -> u8);
	get_volatile!(bist -> u8);

	pub fn has_capabilities(&self) -> bool {
		self.status() & (1 << 4) > 0
	}

	/// Set the flags in the command register.
	pub fn set_command(&self, flags: u16) {
		self.command.set(flags.into());
	}
}

impl fmt::Debug for HeaderCommon {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(HeaderCommon))
			.field("vendor_id", &format_args!("0x{:04x}", self.vendor_id()))
			.field("device_id", &format_args!("0x{:04x}", self.device_id()))
			.field("command", &format_args!("0b{:016b}", self.command()))
			.field("status", &format_args!("0b{:016b}", self.status()))
			.field("revision_id", &self.revision_id())
			.field("prog_if", &self.prog_if())
			.field("subclass", &self.subclass())
			.field("class_code", &self.class_code())
			.field("cache_line_size", &self.cache_line_size())
			.field("latency_timer", &self.latency_timer())
			.field("header_type", &self.header_type())
			.field("bist", &self.bist())
			.finish()
	}
}

/// Header type 0x00
#[repr(C)]
pub struct Header0 {
	pub common: HeaderCommon,

	pub base_address: [BaseAddress; 6],

	cardbus_cis_pointer: VolatileCell<u32le>,

	subsystem_vendor_id: VolatileCell<u16le>,
	subsystem_id: VolatileCell<u16le>,

	expansion_rom_base_address: VolatileCell<u32le>,

	capabilities_pointer: VolatileCell<u8>,

	_reserved: [u8; 7],

	pub interrupt_line: VolatileCell<u8>,
	pub interrupt_pin: VolatileCell<u8>, // TODO is pub a good or bad idea?
	min_grant: VolatileCell<u8>,
	max_latency: VolatileCell<u8>,
}

impl Header0 {
	pub const BASE_ADDRESS_COUNT: u8 = 6;

	/// Return the capability structures attached to this header.
	pub fn capabilities<'a>(&'a self) -> CapabilityIter<'a> {
		CapabilityIter {
			marker: PhantomData,
			next: self.common.has_capabilities().then(|| unsafe {
				let next =
					(self as *const _ as *const u8).add(self.capabilities_pointer.get().into());
				NonNull::new_unchecked(next as *mut Capability).cast()
			}),
		}
	}

	get_volatile!(cardbus_cis_pointer -> u32);
	get_volatile!(subsystem_vendor_id -> u16);
	get_volatile!(subsystem_id -> u16);
	get_volatile!(expansion_rom_base_address -> u32);
	get_volatile!(capabilities_pointer -> u8);
	get_volatile!(interrupt_line -> u8);
	get_volatile!(interrupt_pin -> u8);
	get_volatile!(min_grant -> u8);
	get_volatile!(max_latency -> u8);

	pub fn base_address(&self, index: usize) -> u32 {
		self.base_address[usize::from(index)].get().into()
	}

	pub fn set_base_address(&self, index: usize, value: u32) {
		self.base_address[usize::from(index)].set(value.into());
	}

	pub fn set_command(&self, value: u16) {
		self.common.set_command(value);
	}

	/// Get the full address one or two BARs point to. This may be 64-bit.
	///
	/// Returns `None` if the BAR is invalid.
	pub fn full_base_address(&self, index: usize) -> Option<ParsedBaseAddress> {
		BaseAddress::full_base_address(&self.base_address, index)
	}

	/// Read the status register
	pub fn status(&self) -> u16 {
		self.common.status()
	}
}

impl fmt::Debug for Header0 {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Header0))
			.field("common", &self.common)
			.field("base_address", &self.base_address)
			.field("cardbus_cis_pointer", &self.cardbus_cis_pointer())
			.field(
				"subsystem_vendor_id",
				&format_args!("0x{:04x}", self.subsystem_vendor_id()),
			)
			.field(
				"subsystem_id",
				&format_args!("0x{:04x}", self.subsystem_id()),
			)
			.field(
				"expansion_rom_base_address",
				&format_args!("0x{:08x}", self.expansion_rom_base_address()),
			)
			.field(
				"capabilities_pointer",
				&format_args!("0x{:02x}", self.capabilities_pointer()),
			)
			.field(
				"interrupt_line",
				&format_args!("0x{:02x}", self.interrupt_line()),
			)
			.field(
				"interrupt_pin",
				&format_args!("0x{:02x}", self.interrupt_pin()),
			)
			.field("min_grant", &format_args!("0x{:02x}", self.min_grant()))
			.field("max_latency", &format_args!("0x{:02x}", self.max_latency()))
			.finish()
	}
}

/// Header type 0x01 (Pci-to-PCI bridge)
#[repr(C)]
pub struct Header1 {
	pub common: HeaderCommon,

	pub base_address: [BaseAddress; 2],

	primary_bus_number: VolatileCell<u8>,
	secondary_bus_number: VolatileCell<u8>,
	subordinate_bus_number: VolatileCell<u8>,
	secondary_latency_timer: VolatileCell<u8>,

	io_base: VolatileCell<u8>,
	io_limit: VolatileCell<u8>,
	secondary_status: VolatileCell<u16le>,

	memory_base: VolatileCell<u16le>,
	memory_limit: VolatileCell<u16le>,

	prefetchable_memory_base: VolatileCell<u16le>,
	prefetchable_memory_limit: VolatileCell<u16le>,

	prefetchable_base_upper_32_bits: VolatileCell<u32le>,
	prefetchable_limit_upper_32_bits: VolatileCell<u32le>,

	io_base_upper_16_bits: VolatileCell<u16le>,
	io_limit_upper_16_bits: VolatileCell<u16le>,

	capabilities_pointer: VolatileCell<u8>,

	_reserved: [u8; 3],

	expansion_rom_base_address: VolatileCell<u32le>,

	interrupt_line: VolatileCell<u8>,
	interrupt_pin: VolatileCell<u8>,
	bridge_control: VolatileCell<u16le>,
}

impl Header1 {
	/// Return the capability structures attached to this header.
	pub fn capabilities<'a>(&'a self) -> CapabilityIter<'a> {
		CapabilityIter {
			marker: PhantomData,
			next: self.common.has_capabilities().then(|| unsafe {
				let next =
					(self as *const _ as *const u8).add(self.capabilities_pointer.get().into());
				NonNull::new_unchecked(next as *mut Capability).cast()
			}),
		}
	}

	/// Get the full address one or two BARs point to. This may be 64-bit.
	///
	/// Returns `None` if the BAR is invalid.
	pub fn full_base_address(&self, index: usize) -> Option<ParsedBaseAddress> {
		BaseAddress::full_base_address(&self.base_address, index)
	}
}

impl fmt::Debug for Header1 {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(Header1))
			.field("common", &self.common)
			.field("base_address", &self.base_address)
			.finish_non_exhaustive()
	}
}

/// Enum of possible headers.
#[derive(Clone, Copy, Debug)]
pub enum Header<'a> {
	H0(&'a Header0),
	H1(&'a Header1),
	Unknown(&'a HeaderCommon),
}

impl<'a> Header<'a> {
	pub fn common(&self) -> &'a HeaderCommon {
		match self {
			Self::H0(h) => &h.common,
			Self::H1(h) => &h.common,
			Self::Unknown(hc) => hc,
		}
	}

	pub fn vendor_id(&self) -> u16 {
		self.common().vendor_id.get().into()
	}

	pub fn device_id(&self) -> u16 {
		self.common().device_id.get().into()
	}

	/// Return the capability structures attached to this header.
	pub fn capabilities(&self) -> CapabilityIter<'a> {
		match self {
			Self::H0(h) => h.capabilities(),
			Self::H1(h) => h.capabilities(),
			Self::Unknown(_) => CapabilityIter {
				marker: PhantomData,
				next: None,
			},
		}
	}

	pub fn base_addresses(&self) -> &[BaseAddress] {
		match self {
			Self::H0(h) => &h.base_address[..],
			Self::H1(h) => &h.base_address[..],
			Self::Unknown(_) => &[],
		}
	}

	pub fn full_base_address(&self, index: usize) -> Option<ParsedBaseAddress> {
		BaseAddress::full_base_address(self.base_addresses(), index)
	}

	pub fn header_type(&self) -> u8 {
		self.common().header_type.get()
	}

	pub fn set_command(&self, flags: u16) {
		self.common().set_command(flags);
	}

	/// Read the status register
	pub fn status(&self) -> u16 {
		self.common().status()
	}

	/// The total size of the header, including padding and capabilities region.
	#[inline(always)]
	pub fn size(&self) -> usize {
		1 << 12
	}

	pub unsafe fn from_raw(address: *const ()) -> Self {
		let hc = &*(address as *const HeaderCommon);
		match hc.header_type.get() & 0x7f {
			0 => Self::H0(&*(address as *const Header0)),
			1 => Self::H1(&*(address as *const Header1)),
			_ => Self::Unknown(hc),
		}
	}
}

#[repr(C)]
pub struct Capability {
	id: VolatileCell<u8>,
	next: VolatileCell<u8>,
}

impl Capability {
	/// Return the capability ID.
	pub fn id(&self) -> u8 {
		self.id.get()
	}

	/// Return a reference to data that is located right after the capability header.
	///
	/// ## Safety
	///
	/// It is up to the caller to ensure that the data actually exists and won't go out of bounds.
	pub unsafe fn data<'a, T>(&'a self) -> &'a T {
		&*(self as *const _ as *const u8).cast()
	}

	/// Cast this capability to a concrete type if the ID is recognized.
	pub fn downcast<'a>(&'a self) -> Option<capability::Capability<'a>> {
		unsafe {
			use capability::*;
			match self.id() {
				0x_5 => Some(Capability::Msi(&*(self as *const _ as *const _))),
				0x_9 => Some(Capability::Vendor(&*(self as *const _ as *const _))),
				0x11 => Some(Capability::MsiX(&*(self as *const _ as *const _))),
				_ => None,
			}
		}
	}
}

pub mod capability {
	use super::*;

	pub enum Capability<'a> {
		Msi(&'a Msi),
		Vendor(&'a Vendor),
		MsiX(&'a MsiX),
	}

	impl fmt::Debug for Capability<'_> {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			match self {
				Self::Msi(m) => m.fmt(f),
				Self::Vendor(m) => m.fmt(f),
				Self::MsiX(m) => m.fmt(f),
			}
		}
	}

	#[repr(C)]
	pub struct Msi {
		common: super::Capability,
		message_control: VolatileCell<MsiMessageControl>,
		message_address_low: VolatileCell<u32le>,
		message_address_high: VolatileCell<u32le>,
		message_data: VolatileCell<u16le>,
		_reserved: [u8; 2],
		mask: VolatileCell<u32le>,
		pending: VolatileCell<u32le>,
	}

	#[derive(Clone, Copy)]
	#[repr(transparent)]
	pub struct MsiMessageControl(u16le);

	impl Msi {
		get_volatile!(message_control -> MsiMessageControl);

		#[inline]
		pub fn message_address(&self) -> u64 {
			let f = |n: &VolatileCell<u32le>| u64::from(u32::from(n.get()));
			f(&self.message_address_low) | f(&self.message_address_high) << 32
		}

		get_volatile!(message_data -> u16);
		get_volatile!(mask -> u32);
		get_volatile!(pending -> u32);
	}

	impl MsiMessageControl {
		#[inline]
		pub fn enable(&self) -> bool {
			u16::from(self.0) & 1 > 0
		}

		// TODO other stuff
	}

	impl fmt::Debug for Msi {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct(stringify!(Msi))
				.field("common", &self.common)
				.field("message_control", &self.message_control())
				.field(
					"message_address",
					&format_args!("0x{:016x}", self.message_address()),
				)
				.field(
					"message_data",
					&format_args!("0x{:04x}", self.message_data()),
				)
				.field("mask", &format_args!("0x{:04x}", self.mask()))
				.field("pending", &format_args!("0x{:04x}", self.pending()))
				.finish()
		}
	}

	impl fmt::Debug for MsiMessageControl {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct(stringify!(MsiMessageControl))
				.field("enable", &self.enable())
				.finish_non_exhaustive()
		}
	}

	#[repr(C)]
	pub struct Vendor {
		common: super::Capability,
		length: VolatileCell<u8>,
	}

	impl Vendor {
		get_volatile!(length -> u8);
	}

	impl fmt::Debug for Vendor {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct(stringify!(Vendor))
				.field("common", &self.common)
				.field("length", &self.length())
				.finish_non_exhaustive()
		}
	}

	#[repr(C)]
	pub struct MsiX {
		common: super::Capability,
		message_control: VolatileCell<MsiXMessageControl>,
		table_bir_offset: VolatileCell<u32le>,
		pending_bit_bir_offset: VolatileCell<u32le>,
	}

	#[derive(Clone, Copy)]
	#[repr(transparent)]
	pub struct MsiXMessageControl(u16le);

	impl MsiX {
		get_volatile!(message_control -> MsiXMessageControl);
		set_volatile!(set_message_control: message_control <- MsiXMessageControl);

		#[inline]
		pub fn table(&self) -> (u32, u8) {
			let v = u32::from(self.table_bir_offset.get());
			(v & !0x7, (v & 0x7) as u8)
		}

		#[inline]
		pub fn pending(&self) -> (u32, u8) {
			let v = u32::from(self.pending_bit_bir_offset.get());
			(v & !0x7, (v & 0x7) as u8)
		}
	}

	impl MsiXMessageControl {
		const ENABLE: u16le = u16le::new(1 << 15);

		#[inline]
		pub fn enable(&self) -> bool {
			u16::from(self.0 & Self::ENABLE) > 0u16
		}

		#[inline]
		pub fn set_enable(&mut self, value: bool) {
			if value {
				self.0 |= Self::ENABLE;
			} else {
				self.0 &= !Self::ENABLE;
			}
		}

		#[inline]
		pub fn function_mask(&self) -> bool {
			u16::from(self.0) & (1 << 14) > 0
		}

		#[inline]
		pub fn table_size(&self) -> u16 {
			u16::from(self.0) & 0x3ff
		}
	}

	impl fmt::Debug for MsiX {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			let (table_offset, table_bir) = self.table();
			let (pending_offset, pending_bir) = self.pending();
			f.debug_struct(stringify!(MsiX))
				.field("common", &self.common)
				.field("message_control", &self.message_control())
				.field("table_offset", &format_args!("0x{:04x}", table_offset))
				.field("table_bir", &table_bir)
				.field("pending_offset", &format_args!("0x{:04x}", pending_offset))
				.field("pending_bir", &pending_bir)
				.finish()
		}
	}

	impl fmt::Debug for MsiXMessageControl {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct(stringify!(MsiXMessageControl))
				.field("enable", &self.enable())
				.field("function_mask", &self.function_mask())
				.field("table_size", &self.table_size())
				.finish()
		}
	}
}

impl fmt::Debug for Capability {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(stringify!(Capability))
			.field("id", &format_args!("{:#02x}", self.id()))
			.field("next", &format_args!("{:#02x}", self.id()))
			.finish()
	}
}

pub struct CapabilityIter<'a> {
	next: Option<NonNull<Capability>>,
	marker: PhantomData<&'a Capability>,
}

impl<'a> Iterator for CapabilityIter<'a> {
	type Item = &'a Capability;

	fn next(&mut self) -> Option<Self::Item> {
		self.next.map(|next| unsafe {
			let cap = next.as_ref();
			let offset = usize::from(cap.next.get());
			self.next = if offset != 0 {
				let next = (next.as_ptr() as usize & !0xff) + offset;
				NonNull::new(next as *mut Capability)
			} else {
				None
			};
			cap
		})
	}
}

pub mod msix {
	use super::*;

	#[repr(C)]
	pub struct TableEntry {
		message_address_low: VolatileCell<u32le>,
		message_address_high: VolatileCell<u32le>,
		message_data: VolatileCell<u32le>,
		vector_control: VolatileCell<u32le>,
	}

	impl TableEntry {
		pub fn message_address(&self) -> u64 {
			let f = |n| u64::from(u32::from(n));
			f(self.message_address_low.get()) | f(self.message_address_high.get()) << 32
		}

		pub fn set_message_address(&self, address: u64) {
			self.message_address_low.set((address as u32).into());
			self.message_address_high
				.set(((address >> 32) as u32).into());
		}

		get_volatile!(message_data -> u32);
		set_volatile!(set_message_data: message_data <- u32);

		pub fn is_vector_control_masked(&self) -> bool {
			u32::from(self.vector_control.get()) & 1 > 0
		}

		pub fn set_vector_control_mask(&self, mask: bool) {
			self.vector_control.set(u32::from(mask).into())
		}
	}

	impl fmt::Debug for TableEntry {
		fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
			f.debug_struct(stringify!(TableEntry))
				.field(
					"message_address",
					&format_args!("0x{:016x}", self.message_address()),
				)
				.field(
					"message_data",
					&format_args!("0x{:08x}", self.message_data()),
				)
				.field("is_vector_control_masked", &self.is_vector_control_masked())
				.finish()
		}
	}
}

/// Representation of a Pci MMIO area
pub struct Pci {
	/// The start of the area
	start: NonNull<()>,
	/// The physical address of the area.
	physical_address: usize,
	/// The size of the area in bytes
	_size: usize,
	/// MMIO ranges for use with base addresses
	mem: [Option<PhysicalMemory>; 8],
	/// Ugly hacky but working counter for MMIO bump allocator.
	alloc_counter: Cell<usize>,
}

impl Pci {
	/// Create a new Pci MMIO wrapper.
	///
	/// `start` and `size` refer to the Pci configuration sections while `mmio` refers to the
	/// areas that can be allocated for use with base addresses.
	///
	/// ## Safety
	///
	/// The range must map to a valid PCI MMIO area.
	pub unsafe fn new(
		start: NonNull<()>,
		physical_address: usize,
		size: usize,
		mem: &[PhysicalMemory],
	) -> Self {
		let mut mm = [None; 8];
		for (i, m) in mem.iter().copied().enumerate() {
			mm[i] = Some(m);
		}
		let mem = mm;
		let alloc_counter = Cell::new(0);
		Self {
			start,
			physical_address,
			_size: size,
			mem,
			alloc_counter,
		}
	}

	/// Returns an iterator over all the valid devices.
	pub fn iter<'a>(&'a self) -> IterPci<'a> {
		IterPci { pci: self, bus: 0 }
	}

	/// Return a reference to the configuration header for a function.
	///
	/// Returns `None` if `vendor_id == 0xffff`.
	///
	/// ## Panics
	///
	/// If the bus + device + function are out of the MMIO range.
	pub fn get(&self, bus: u8, device: u8, function: u8) -> Option<Header> {
		let h = self.get_unchecked(bus, device, function);
		if h.common().vendor_id.get() == 0xffff.into() {
			None
		} else {
			Some(h)
		}
	}

	/// Return the physical address of the configuration header for a function.
	///
	/// Useful if passing to a separate driver task.
	///
	/// ## Panics
	///
	/// If either the device or function are out of bounds.
	pub fn get_physical_address(&self, bus: u8, device: u8, function: u8) -> usize {
		self.physical_address + Self::offset(bus, device, function)
	}

	/// Return the child address of a function.
	///
	/// ## Panics
	///
	/// If either the device or function are out of bounds.
	#[inline(always)]
	fn get_child_address(&self, bus: u8, device: u8, function: u8) -> u32 {
		(Self::offset(bus, device, function) >> 4)
			.try_into()
			.unwrap()
	}

	/// Return the byte offset for a function configuration area.
	///
	/// ## Panics
	///
	/// If either the device or function are out of bounds.
	fn offset(bus: u8, device: u8, function: u8) -> usize {
		assert!(device < 32 && function < 8);
		(usize::from(bus) << 20) | (usize::from(device) << 15) | (usize::from(function) << 12)
	}

	/// Return a reference to the configuration header for a function. This won't
	/// return `None`, but the header values may be all `1`s.
	///
	/// ## Panics
	///
	/// If either the device or function are out of bounds.
	fn get_unchecked<'a>(&'a self, bus: u8, device: u8, function: u8) -> Header<'a> {
		let offt = Self::offset(bus, device, function);
		unsafe {
			let h = self.start.as_ptr().cast::<u8>().add(offt);
			let hc = &*h.cast::<HeaderCommon>();
			match hc.header_type.get() & 0x7f {
				0 => Header::H0(&*h.cast()),
				1 => Header::H1(&*h.cast()),
				_ => Header::Unknown(hc),
			}
		}
	}

	/// Return a region of MMIO.
	///
	/// ## Notes
	///
	/// Currently all memory will be 16K byte aligned. Higher granulity will be supported later.
	pub fn allocate_mmio(&self, size: usize, _flags: u8) -> Result<Mmio<'_>, ()> {
		assert!(size <= 1 << 16, "TODO");
		let size = 1 << 16;
		let c = self.alloc_counter.get();
		self.alloc_counter.set(c + size);
		Ok(Mmio {
			physical: self.mem[0].unwrap().physical + c,
			virt: NonNull::new(self.mem[0].unwrap().virt.as_ptr().wrapping_add(c))
				.unwrap()
				.cast(),
			size,
			_pci: self,
		})
	}
}

/// A physically contiguous memory region.
#[derive(Clone, Copy)]
pub struct PhysicalMemory {
	/// The physical address
	pub physical: usize,
	/// The virtual address
	pub virt: NonNull<()>,
	/// The size in bytes
	pub size: usize,
}

impl fmt::Debug for PhysicalMemory {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(PhysicalMemory))
			.field("physical", &format_args!("0x{:x}", self.physical))
			.field("virt", &self.virt)
			.field("size", &format_args!("0x{:x}", self.size))
			.finish()
	}
}

/// A MMIO region
pub struct Mmio<'a> {
	/// The physical address
	pub physical: usize,
	/// The virtual address
	pub virt: NonNull<u8>,
	/// The size in bytes
	pub size: usize,
	/// The Pci device this region belongs to.
	_pci: &'a Pci,
}

/// A specific Pci bus.
pub struct Bus<'a> {
	pci: &'a Pci,
	bus: u8,
}

impl<'a> Bus<'a> {
	pub fn iter(&self) -> IterBus<'a> {
		IterBus {
			pci: self.pci,
			bus: self.bus,
			device: 0,
		}
	}
}

impl<'a> From<Bus<'a>> for Option<Header<'a>> {
	fn from(f: Bus<'a>) -> Self {
		f.pci.get(f.bus, 0, 0)
	}
}

/// A specific Pci device.
pub struct Device<'a> {
	pci: &'a Pci,
	bus: u8,
	device: u8,
}

impl<'a> Device<'a> {
	#[inline]
	pub fn bus(&self) -> u8 {
		self.bus
	}

	#[inline]
	pub fn device(&self) -> u8 {
		self.device
	}

	#[inline]
	pub fn vendor_id(&self) -> u16 {
		self.header().common().vendor_id.get().into()
	}

	#[inline]
	pub fn device_id(&self) -> u16 {
		self.header().common().device_id.get().into()
	}

	#[inline]
	pub fn header(&self) -> Header {
		self.pci.get_unchecked(self.bus, self.device, 0)
	}

	#[inline]
	pub fn header_physical_address(&self) -> usize {
		self.pci.get_physical_address(self.bus, self.device, 0)
	}

	#[inline]
	pub fn child_address(&self) -> u32 {
		self.pci.get_child_address(self.bus, self.device, 0)
	}
}

impl fmt::Debug for Device<'_> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("Device")
			.field("vendor_id", &format_args!("0x{:x}", self.vendor_id()))
			.field("device_id", &format_args!("0x{:x}", self.device_id()))
			.field("location", &format_args!("{} -> {}", self.bus, self.device))
			.finish_non_exhaustive()
	}
}

impl<'a> From<Device<'a>> for Option<Header<'a>> {
	fn from(f: Device<'a>) -> Self {
		f.pci.get(f.bus, f.device, 0)
	}
}

/// A specific Pci function.
pub struct Function<'a> {
	pci: &'a Pci,
	bus: u8,
	device: u8,
	function: u8,
}

impl<'a> From<Function<'a>> for Option<Header<'a>> {
	fn from(f: Function<'a>) -> Self {
		f.pci.get(f.bus, f.device, f.function)
	}
}

pub struct IterPci<'a> {
	pci: &'a Pci,
	bus: u8,
}

pub struct IterBus<'a> {
	pci: &'a Pci,
	bus: u8,
	device: u8,
}

pub struct IterDevice<'a> {
	pci: &'a Pci,
	bus: u8,
	device: u8,
	function: u8,
}

impl<'a> Iterator for IterPci<'a> {
	type Item = Bus<'a>;

	fn next(&mut self) -> Option<Bus<'a>> {
		if self.bus == 0xff {
			return None;
		} else if self.bus == 0 {
			let h = self.pci.get_unchecked(0, 0, 0);
			if h.common().header_type.get() & 0x80 == 0 {
				self.bus = 0xff;
				return Some(Bus {
					pci: self.pci,
					bus: 0,
				});
			}
		}

		self.bus += 1;
		let h = self.pci.get_unchecked(0, 0, self.bus);
		if h.common().vendor_id.get() != 0xffff.into() {
			self.bus = 0xff;
			None
		} else {
			Some(Bus {
				pci: self.pci,
				bus: self.bus,
			})
		}
	}
}

impl<'a> Iterator for IterBus<'a> {
	type Item = Device<'a>;

	fn next(&mut self) -> Option<Device<'a>> {
		while self.device < 32 {
			let dev = self.device;
			self.device += 1;
			if self.pci.get(self.bus, dev, 0).is_some() {
				return Some(Device {
					pci: self.pci,
					bus: self.bus,
					device: dev,
				});
			}
		}
		None
	}
}

pub enum FunctionItem<'a> {
	Header(Header<'a>),
	Bus(Bus<'a>),
}

impl<'a> Iterator for IterDevice<'a> {
	type Item = FunctionItem<'a>;

	fn next(&mut self) -> Option<FunctionItem<'a>> {
		if self.function == 0xff {
			None
		} else {
			let h = self.pci.get_unchecked(self.bus, self.device, self.function);
			if h.common().vendor_id.get() == 0xffff.into() {
				self.function = 0xff;
				None
			} else {
				let ht = h.common().header_type.get();
				if ht & 0x80 > 0 {
					if let Header::H1(h) = h {
						if h.common.class_code.get() == 0x6 && h.common.subclass.get() == 0x4 {
							let sb = h.secondary_bus_number.get();
							Some(FunctionItem::Bus(Bus {
								pci: self.pci,
								bus: sb,
							}))
						} else {
							Some(FunctionItem::Header(Header::H1(h)))
						}
					} else {
						Some(FunctionItem::Header(h))
					}
				} else {
					Some(FunctionItem::Header(h))
				}
			}
		}
	}
}
