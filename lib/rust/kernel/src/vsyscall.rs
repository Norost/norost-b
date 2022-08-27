use core::{
	arch::x86_64,
	hint,
	ptr::NonNull,
	sync::atomic::{AtomicI8, AtomicU32, AtomicU64, AtomicU8, Ordering},
};

pub const VSYSCALL_DATA: NonNull<VsyscallData> = unsafe { NonNull::new_unchecked(0x1000 as _) };

pub fn vsyscall_data() -> &'static VsyscallData {
	unsafe { VSYSCALL_DATA.as_ref() }
}

#[repr(C)]
pub struct VsyscallData {
	pub time_info: TimeInfo,
}

#[repr(C)]
pub struct TimeInfo {
	pub version: AtomicU32,
	pub _reserved_0: AtomicU32,
	pub tsc_timestamp: AtomicU64,
	pub system_time: AtomicU64,
	pub tsc_to_system_mul: AtomicU32,
	pub tsc_shift: AtomicI8,
	pub flags: AtomicU8,
	pub _reserved_1: [AtomicU8; 2],
}

impl TimeInfo {
	pub fn now_nanos(&self) -> u64 {
		loop {
			// Check if we can read the parameters or if we should wait.
			let v = self.version.load(Ordering::Acquire);
			if v & 1 != 0 {
				hint::spin_loop();
				continue;
			}

			let t = unsafe { x86_64::_rdtsc() };
			let t = t.wrapping_add(self.tsc_timestamp.load(Ordering::Relaxed));
			let tsc_shift = self.tsc_shift.load(Ordering::Relaxed);
			let t = if tsc_shift >= 0 {
				t.wrapping_shl(tsc_shift as _)
			} else {
				t.wrapping_shr(tsc_shift.wrapping_neg() as _)
			};
			let t = ((u128::from(t) * u128::from(self.tsc_to_system_mul.load(Ordering::Relaxed)))
				>> 32) as u64;
			let t = t.wrapping_add(self.system_time.load(Ordering::Relaxed));

			// If self got updated during calculations, try again
			if v == self.version.load(Ordering::Acquire) {
				break t;
			}
		}
	}
}
