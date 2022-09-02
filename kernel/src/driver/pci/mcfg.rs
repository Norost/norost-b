// Copied from acpi crate because I can't be bothered maintaining yet another fork.

use {
	acpi::{sdt::SdtHeader, AcpiTable},
	core::{mem, slice},
};

// Use u8 only as QEMU puts this structure at address 0x0ffe2269 for a presumably cursed reason
#[repr(C)]
pub struct Mcfg {
	header: SdtHeader,
	_reserved: [u8; 8],
	// Followed by `n` entries with format `McfgEntry`
}

impl AcpiTable for Mcfg {
	fn header(&self) -> &SdtHeader {
		&self.header
	}
}

impl Mcfg {
	pub fn entries(&self) -> &[McfgEntry] {
		let length = self.header.length as usize - mem::size_of::<Mcfg>();

		// Intentionally round down in case length isn't an exact multiple of McfgEntry size
		// (see rust-osdev/acpi#58)
		let num_entries = length / mem::size_of::<McfgEntry>();

		unsafe {
			let pointer =
				(self as *const Mcfg as *const u8).add(mem::size_of::<Mcfg>()) as *const McfgEntry;
			slice::from_raw_parts(pointer, num_entries)
		}
	}
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct McfgEntry {
	base_address: [u8; 8],
	_pci_segment_group: [u8; 2],
	pub bus_number_start: u8,
	pub bus_number_end: u8,
	_reserved: [u8; 4],
}

impl McfgEntry {
	pub fn base_address(&self) -> u64 {
		u64::from_le_bytes(self.base_address)
	}
}
