use core::{
	arch::x86_64::{_xrstor64, _xsave64, _xsetbv},
	mem,
};

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

const MXCSR_PRECISION_MASK: u32 = 1 << 12;
const MXCSR_UNDERFLOW_MASK: u32 = 1 << 11;
const MXCSR_OVERFLOW_MASK: u32 = 1 << 10;
const MXCSR_DIVIDE_BY_ZERO_MASK: u32 = 1 << 9;
const MXCSR_DENORMAL_OPERATION_MASK: u32 = 1 << 8;
const MXCSR_INVALID_OPERATION_MASK: u32 = 1 << 8;

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

#[repr(C)]
pub struct LegacyRegion {
	_stuff: u128,
	fpu_dp: u64,
	mxcsr: u32,
	mxcsr_mask: u32,
	mm: [u128; 8],
	xmm: [u128; 16],
	_reserved: [u128; 6],
}

const _: () = assert!(mem::size_of::<LegacyRegion>() == 512);

impl Default for LegacyRegion {
	fn default() -> Self {
		Self {
			_stuff: 0,
			fpu_dp: 0,
			// default MXCSR on Linux (doing nothing but execute divss in a loop):
			//   (gdb) p $mxcsr
			//   $1 = [ IE IM DM ZM OM UM PM ]
			// without divss:
			//   $1 = [ IM DM ZM OM UM PM ]
			mxcsr: MXCSR_INVALID_OPERATION_MASK
				| MXCSR_DENORMAL_OPERATION_MASK
				| MXCSR_DIVIDE_BY_ZERO_MASK
				| MXCSR_OVERFLOW_MASK
				| MXCSR_UNDERFLOW_MASK
				| MXCSR_PRECISION_MASK,
			mxcsr_mask: 0,
			mm: [0; 8],
			xmm: [0; 16],
			_reserved: [0; 6],
		}
	}
}

#[derive(Default)]
#[repr(C)]
pub struct XSaveHeader([u128; 4]);

#[derive(Default)]
#[repr(C)]
pub struct AvxRegion([u128; 16]);

extern "C" fn handle_device_not_available(_rip: *const ()) {
	panic!("FPU should be unconditionally enabled");
}

/// # Safety
///
/// May only be called once at boot time.
pub unsafe fn init(feat: &super::cpuid::Features) {
	use super::*;
	unsafe {
		idt_set(7, wrap_idt!(rip handle_device_not_available));
		let mut flags = X87_STATE | SSE_STATE;
		flags |= u64::from(feat.avx2()) * AVX_STATE;
		_xsetbv(0, flags);
	}
}
