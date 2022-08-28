use crate::memory::frame::PPN;

const DATA_USER_ADDR: u64 = 0x1000;
static mut DATA_PHYS_ADDR: PPN = PPN(0);

/// # Safety
///
/// This function may only be called once at boot time.
pub unsafe fn init(boot: &crate::boot::Info) {
	// SAFETY: only we are accessing DATA_PHYS_ADDR at this moment.
	unsafe { DATA_PHYS_ADDR = PPN::try_from_usize(boot.vsyscall_phys_addr as _).unwrap() }
}

pub struct Mapping {
	pub data_virt_addr: u64,
	pub data_phys_addr: PPN,
}

pub fn mapping() -> Mapping {
	Mapping {
		data_virt_addr: DATA_USER_ADDR,
		// SAFETY: nothing will write to DATA_PHYS_ADDR after boot.
		data_phys_addr: unsafe { DATA_PHYS_ADDR },
	}
}
