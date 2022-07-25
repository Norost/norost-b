use super::syscall::current_thread_ptr;
use alloc::boxed::Box;
use core::arch::x86_64::{_xgetbv, _xrstor64, _xsave64, _xsetbv};

const X87_STATE: u64 = 1 << 0;

const SSE_STATE: u64 = 1 << 1;

const AVX_STATE: u64 = 1 << 2;

#[allow(dead_code)]
const MPX_BNDREGS_STATE: u8 = 3;
#[allow(dead_code)]
const MPX_BNDCSR_STATE: u8 = 4;

#[allow(dead_code)]
const AVX512_OPMASK_STATE: u8 = 5;
#[allow(dead_code)]
const AVX512_ZMM_HI256_STATE: u8 = 6;
#[allow(dead_code)]
const AVX512_HI16_ZMM_STATE: u8 = 7;

#[allow(dead_code)]
const PT_STATE: u8 = 8;

#[allow(dead_code)]
const PKRU_STATE: u8 = 9;

#[allow(dead_code)]
const CET_U_STATE: u8 = 11;
#[allow(dead_code)]
const CET_S_STATE: u8 = 12;

#[allow(dead_code)]
const HDC_STATE: u8 = 13;

#[allow(dead_code)]
const LBR_STATE: u8 = 15;

#[allow(dead_code)]
const HWP_STATE: u8 = 16;

#[allow(dead_code)]
const XCOMP_BV: u8 = 63;

// Keep things simple and just save everything.
//
// I originally tried to have separate states for XMM/YMM/... to optimize memory usage but
// it proved to be too ardous and no other kernels seem to do it, so we'll do it the simple,
// stupid way.
//
// In fact, since basically every program uses SSE2 (and higher) anyways, we won't make it
// optional and unconditionally initialize the FPU, which should be net faster.
#[derive(Default)]
#[repr(align(64))]
#[repr(C)]
pub struct FloatStorage {
	legacy: LegacyRegion,
	header: XSaveHeader,
	avx: AvxRegion,
}

impl FloatStorage {
	pub fn save(&mut self) {
		unsafe {
			_xsave64(self as *mut _ as _, u64::MAX);
		}
	}

	/// Restore XSAVE state.
	///
	/// This clears the CR0.TS bit.
	pub fn restore(&self) {
		unsafe {
			super::cpuid::clear_task_switch();
			_xrstor64(self as *const _ as _, u64::MAX);
		}
	}
}

#[derive(Default)]
pub struct LegacyRegion([u128; 32]);

#[derive(Default)]
pub struct XSaveHeader([u128; 4]);

#[derive(Default)]
pub struct AvxRegion([u128; 16]);

extern "C" fn handle_device_not_available(_rip: *const ()) {
	panic!("FPU should be unconditionally enabled");
}

/// # Safety
///
/// May only be called once at boot time.
pub unsafe fn init() {
	use super::*;
	unsafe {
		idt_set(7, wrap_idt!(rip handle_device_not_available));
		_xsetbv(0, X87_STATE | SSE_STATE | AVX_STATE);
	}
}
