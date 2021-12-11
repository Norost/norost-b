use crate::memory::r#virtual::add_identity_mapping;
use crate::time::Monotonic;
use core::cell::UnsafeCell;
use core::{ptr, fmt};
use acpi::{AcpiTables, AcpiHandler, hpet::HpetInfo};

// No atomic is strictly necessary since we only read from this after boot.
static mut ADDRESS: *const Hpet = core::ptr::null();
// Ditto
static mut MULTIPLIER: u128 = 0;

impl Monotonic {
	pub fn now() -> Self {
		let m = unsafe { MULTIPLIER };
		Self::from_nanoseconds(u128::from(hpet().counter.get()) * m)
	}
}

#[repr(C)]
pub struct Hpet {
	capabilities_id: Reg,
	configuration: Reg,
	interrupt_status: Reg,
	_reserved: [Reg; 0xc],
	pub counter: Reg,
}

impl Hpet {
	fn capabilities_id(&self) -> CapabilitiesId {
		CapabilitiesId(self.capabilities_id.get())
	}
}

impl fmt::Debug for Hpet {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_struct(stringify!(Hpet));
		let cap = self.capabilities_id.get();
		f.field("period", &(cap >> 32));
		f.field("vendor_id", &format_args!("{:#x}", (cap >> 16) as u16));
		f.field("capabilities", &format_args!("{:#x}", cap & 0xffff_ffff));
		f.field("configuration", &format_args!("{:#x}", self.configuration.get()));
		f.field("interrupt_status", &format_args!("{:#x}", self.interrupt_status.get()));
		f.field("counter", &self.counter.get());
		f.finish()
	}
}

#[repr(transparent)]
pub struct CapabilitiesId(u64);

impl CapabilitiesId {
	pub fn period(&self) -> u32 {
		(self.0 >> 32) as u32
	}
}

#[allow(dead_code)]
#[repr(C)]
pub struct Timer {
	configuration_capabilities: Reg,
	comparator_value: Reg,
	fsb_interrupt_route: Reg,
}

#[repr(C)]
pub struct Reg {
	value: UnsafeCell<u64>,
	_reserved: u64,
}

impl Reg {
	pub fn get(&self) -> u64 {
		unsafe { ptr::read_volatile(self.value.get()) }
	}

	pub fn set(&self, value: u64) {
		unsafe { ptr::write_volatile(self.value.get(), value) }
	}
}

pub(super) fn init_acpi<H>(acpi: &AcpiTables<H>)
where
	H: AcpiHandler,
{
	let h = HpetInfo::new(acpi).unwrap();
	assert!(h.main_counter_is_64bits());
	unsafe {
		ADDRESS = add_identity_mapping(h.base_address.try_into().unwrap(), 4096).unwrap().cast().as_ptr();
		// Period is in femtoseconds.
		MULTIPLIER = u128::from(hpet().capabilities_id().period()) / 1_000_000;
	}
	hpet().configuration.set(hpet().configuration.get() | 1);
}

pub fn hpet() -> &'static Hpet {
	unsafe { &*ADDRESS }
}
