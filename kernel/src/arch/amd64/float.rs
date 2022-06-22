use super::syscall::current_thread_ptr;
use alloc::boxed::Box;
use core::arch::{
	asm,
	x86_64::{_xgetbv, _xrstor64, _xsave64, _xsetbv},
};

#[allow(dead_code)]
const X87_STATE: u8 = 0;

const SSE_STATE: u8 = 1;

#[allow(dead_code)]
const AVX_STATE: u8 = 2;

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

const SUPPORT_MASK: u64 = 1 << SSE_STATE;

#[derive(Default)]
pub enum FloatStorage {
	#[default]
	None,
	Xmm(Box<Xmm>),
}

impl FloatStorage {
	pub fn save(&mut self) {
		match self {
			Self::None => {}
			Self::Xmm(xmm) => xmm.save(),
		}
	}

	pub fn restore(&self) {
		match self {
			Self::None => {}
			Self::Xmm(xmm) => xmm.restore(),
		}
	}
}

#[derive(Default)]
#[repr(align(64))]
pub struct LegacyRegion([u128; 32]);

#[derive(Default)]
#[repr(align(64))]
pub struct XSaveHeader([u128; 4]);

#[derive(Default)]
#[repr(align(64))]
#[repr(C)]
pub struct Xmm(LegacyRegion, XSaveHeader);

impl Xmm {
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

struct Xcr0(u64);

impl Xcr0 {
	fn load() -> Self {
		unsafe { Self(_xgetbv(0)) }
	}

	fn cleared() -> Self {
		Self(0)
	}

	fn enable(&mut self, feature: u8) {
		self.0 |= 1 << feature;
	}

	unsafe fn store(&self) {
		unsafe { _xsetbv(0, self.0) }
	}
}

extern "C" fn handle_device_not_available(_rip: *const ()) {
	let fp = unsafe {
		&mut *current_thread_ptr()
			.expect("no thread running")
			.as_ref()
			.arch_specific
			.float
			.get()
	};
	match fp {
		FloatStorage::None => {
			// Box::default() uses the box keyword internally, which makes it much less
			// likely the compiler stupidly reserves a huge amount of stack space.
			let xmm = Box::<Xmm>::default();
			xmm.restore();
			*fp = FloatStorage::Xmm(xmm);
		}
		FloatStorage::Xmm(xmm) => xmm.restore(),
	}
}

/// # Safety
///
/// May only be called once at boot time.
pub unsafe fn init() {
	use super::*;
	unsafe {
		idt_set(7, wrap_idt!(rip handle_device_not_available));
	}
}
