const INTEL: u16 = 0x8086;

/// # Safety
///
/// `vendor` and `device` must be correct.
///
/// `pci` must point to the start of the PCI function of the xHC.
pub unsafe fn apply(vendor: u16, _device: u16, pci: *mut u8) {
	if vendor != INTEL {
		return;
	}

	enable_xhci_ports(pci);
}

/// Switch ports over from EHC to xHC.
///
/// Some chips feature both an EHC and xHC which share ports.
/// Whether they are accessible by the EHC or xHC is determined by a hardware toggle.
///
/// Chips which have both EHCI and xHCI are:
///
/// * Intel Panther Point
///
/// # Safety
///
/// `pci` must point to the start of the PCI function of the xHC.
///
/// # References
///
/// Commit `69e848c2090aebba5698a1620604c7dccb448684` ("Intel xhci: Support EHCI/xHCI port
/// switching.") in the Linux source tree.
unsafe fn enable_xhci_ports(pci: *mut u8) {
	info!("Switch EHC ports to xHC");

	const USB3_PSSEN: usize = 0xd0;
	const XUSB2PR: usize = 0xd8;

	// Enable SuperSpeed on USB3 ports
	pci.add(USB3_PSSEN).cast::<u32>().write_volatile(u32::MAX);

	// Switch over USB2 ports
	pci.add(XUSB2PR).cast::<u32>().write_volatile(u32::MAX);
}
