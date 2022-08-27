//! # pvclock driver
//!
//! ## References
//!
//! * https://docs.kernel.org/virt/kvm/x86/msr.html

use {
	super::cpuid,
	crate::{boot, driver::hpet, time::Monotonic},
	core::{
		arch::x86_64,
		sync::atomic::{AtomicI8, AtomicU32, AtomicU64, AtomicU8, Ordering},
	},
	norostb_kernel::vsyscall::TimeInfo,
};

pub const MSR_KVM_SYSTEM_TIME_NEW: u32 = 0x4b564d01;

#[link_section = ".vsyscall.data.timeinfo"]
static TIME_INFO: TimeInfo = TimeInfo {
	version: AtomicU32::new(0),
	_reserved_0: AtomicU32::new(0),
	tsc_timestamp: AtomicU64::new(0),
	system_time: AtomicU64::new(0),
	tsc_to_system_mul: AtomicU32::new(0),
	tsc_shift: AtomicI8::new(0),
	flags: AtomicU8::new(0),
	_reserved_1: [AtomicU8::new(0), AtomicU8::new(0)],
};

pub fn init(boot: &boot::Info, features: &cpuid::Features) {
	if features.kvm_feature_clocksource2() {
		debug_assert!(
			boot.vsyscall_phys_addr != 0,
			"boot loader did not set vsyscall page address"
		);
		let offset = &TIME_INFO as *const _ as u64 & 0xfff;
		unsafe {
			crate::arch::msr::wrmsr(
				MSR_KVM_SYSTEM_TIME_NEW,
				u64::from(boot.vsyscall_phys_addr) | offset | 1,
			);
		}
	} else {
		// Calibrate manually
		let dt_ns = 10_000_000;
		let end = hpet::now().saturating_add_nanos(dt_ns);
		let mut v = 0;
		let t = unsafe { x86_64::__rdtscp(&mut v) };
		while hpet::now() < end { /* pass */ }
		let dt = unsafe { x86_64::__rdtscp(&mut v) } - t;
		let mut tsc_to_system_mul = (u128::from(dt_ns) << 32) / u128::from(dt);
		let mut tsc_shift = 0;
		let mut tsc_to_system_mul = loop {
			if let Ok(n) = u32::try_from(tsc_to_system_mul) {
				break n;
			} else {
				tsc_to_system_mul /= 2;
				tsc_shift += 1;
			}
		};
		while let Some(n) = tsc_to_system_mul.checked_mul(2) {
			tsc_to_system_mul = n;
			tsc_shift -= 1;
		}
		TIME_INFO
			.tsc_to_system_mul
			.store(tsc_to_system_mul, Ordering::Relaxed);
		TIME_INFO.tsc_shift.store(tsc_shift, Ordering::Relaxed);
	}
}

impl Monotonic {
	pub fn now() -> Self {
		Self::from_nanos(TIME_INFO.now_nanos())
	}
}
