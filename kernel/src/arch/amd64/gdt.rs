use super::tss::TSS;
use core::convert::TryInto;
use core::marker::PhantomData;
use core::mem;
use core::pin::Pin;

// ~~stolen from~~ inspired by ToaruOS code

#[repr(C)]
#[repr(packed)]
struct GDTEntry {
	limit_low: u16,
	base_low: u16,
	base_mid: u8,
	/// `access` bits:
	/// * 0:0 accessed (set by CPU)
	/// * 1:1
	///   * data: writeable (0 = read-only, 1 = read-write)
	///   * code: readable (0 = execute-only, 1 = read-execute)
	/// * 2:2
	///   * data: direction (0 = grows up & limit > offset, 1 = grows down & limit < offset)
	///   * code: conforming (0 = ring set == privilege, 1 = ring set >= privilege)
	/// * 3:3 executable (1 = code, 0 = data)
	/// * 4:4 descriptor type (1 = code/data, 0 = everything else)
	/// * 5:7 privilege (0 = kernel, 3 = user)
	/// * 7:7 present
	access: u8,
	/// `granularity` bits:
	/// * 0:3 limit
	/// * 4:4 unused
	/// * 5:5 long mode descriptor (x86-64 only)
	/// * 6:6 size bit (0 = 16-bit protected mode, 1 = 32-bit protected mode)
	///   * x86-64: must be 0 if long mode descriptor = 1
	granularity: u8,
	base_high: u8,
}

#[repr(C)]
pub struct GDTEntryHigh {
	base_higher: u32,
	_reserved: u32,
}

#[repr(C)]
pub struct GDTPointer {
	_padding: [u16; 3],
	limit: u16,
	address: u64,
}

#[repr(C)]
pub struct GDT<'a> {
	null: GDTEntry,
	kernel_code: GDTEntry,
	kernel_data: GDTEntry,
	user_code: GDTEntry,
	user_data: GDTEntry,
	tss: GDTEntry,
	tss_extra: GDTEntryHigh,
	_tss_marker: PhantomData<&'a ()>,
}

impl<'a> GDT<'a> {
	pub fn new(tss: &'a TSS) -> Self {
		let tss = tss as *const _ as u64;

		// Layout
		//
		// 0: null
		// 1: kernel code
		// 2: kernel data
		// 3: user code
		// 4: user data
		// 5 & 6: tss
		//
		// See syscall::init() for reasoning
		Self {
			null: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0,
				granularity: 1,
				base_high: 0,
			},
			kernel_code: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1_00_1_1_0_1_0,
				granularity: (1 << 5) | (1 << 7) | 0xf,
				base_high: 0,
			},
			kernel_data: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1_00_1_0_0_1_0,
				granularity: (1 << 5) | (1 << 7) | 0xf,
				base_high: 0,
			},
			user_data: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1_11_1_0_0_1_0,
				granularity: (1 << 5) | (1 << 7) | 0xf,
				base_high: 0,
			},
			user_code: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1_11_1_1_0_1_0,
				granularity: (1 << 5) | (1 << 7) | 0xf,
				base_high: 0,
			},
			tss: GDTEntry {
				limit_low: 0x0067,
				base_low: (tss >> 0) as u16,
				base_mid: (tss >> 16) as u8,
				access: 0xe9,
				granularity: (1 << 5) | (1 << 7) | 0xf,
				base_high: (tss >> 24) as u8,
			},
			tss_extra: GDTEntryHigh {
				base_higher: (tss >> 32) as u32,
				_reserved: 0,
			},
			_tss_marker: PhantomData,
		}
	}
}

impl GDTPointer {
	pub fn new(gdt: Pin<&GDT>) -> Self {
		unsafe {
			Self {
				_padding: [0; 3],
				limit: (mem::size_of::<GDT>() - 1).try_into().unwrap(),
				address: gdt.get_ref() as *const _ as u64,
			}
		}
	}

	pub unsafe fn activate(&self) {
		asm!("
			lgdt	[{0}]

			lea		rax, [rip + .reload_gdt]
			push	0x8					# cs
			push	rax					# rîp
			rex64 retf

		.reload_gdt:
			mov		ax, 2 * 8
			mov		ds, ax
			mov		es, ax
			mov		fs, ax
			mov		gs, ax
			mov		ss, ax
			
			mov		ax, 5 * 8
			ltr		ax
		", in(reg) &self.limit);
	}
}
