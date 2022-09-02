//! # Vendor-specific functionality

mod intel;

/// Apply vendor-specific fixes.
///
/// # Safety
///
/// `vendor` and `device` must be correct.
///
/// `pci` must point to the start of the PCI function of the xHC.
pub unsafe fn apply(vendor: u16, device: u16, pci: *mut u8) {
	intel::apply(vendor, device, pci);
}
