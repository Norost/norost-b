use super::tss::TSS;
use core::arch::asm;
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
	/// `access` bits for data segments:
	/// * 0:0 accessed (set by CPU)
	/// * 1:1
	///   * data: writeable (0 = read-only, 1 = read-write)
	///   * code: readable (0 = execute-only, 1 = read-execute)
	/// * 2:2
	///   * data: direction (0 = grows up & limit > offset, 1 = grows down & limit < offset)
	///   * code: conforming (0 = ring set == privilege, 1 = ring set >= privilege)
	/// * 3:3 executable (1 = code, 0 = data)
	/// * 4:4 descriptor type (1 = code/data, 0 = system)
	/// * 5:7 privilege (0 = kernel, 3 = user)
	/// * 7:7 present
	///
	/// `access` bits for system segments:
	/// * 0:3 type (0x2 = LDT, 0x9 = 64-bit TSS available, 0xB = 64-bit TSS busy)
	/// * 4:4 descriptor type (1 = code/data, 0 = system)
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
	null: GDTEntry,        // 0
	kernel_code: GDTEntry, // 1
	kernel_data: GDTEntry, // 2
	user_data: GDTEntry,   // 3
	user_code: GDTEntry,   // 4
	tss: GDTEntry,         // 5
	tss_extra: GDTEntryHigh,
	_marker: PhantomData<&'a ()>,
}

impl<'a> GDT<'a> {
	pub const KERNEL_CS: u16 = 8 * 1 | 0;
	pub const KERNEL_SS: u16 = 8 * 2 | 0;
	pub const USER_CS: u16 = 8 * 4 | 3;
	pub const USER_SS: u16 = 8 * 3 | 3;
	pub const TSS: u16 = 5 * 8 | 3;

	pub fn new(tss: &'a TSS) -> Self {
		let tss = tss as *const _ as u64;

		// Layout
		//
		// 0: null
		// 1: kernel code
		// 2: kernel data
		// 3: user data
		// 4: user code
		// 5: tss
		//
		// Reasoning:
		// - 0 always has to be a null segment.
		// - kernel data has to come after kernel code because of SYSCALL
		// - user data has to come *before* kernel code because of SYSRET (thanks AMD!)
		// - TSS is necessary because of RSP when an interrupt switches from ring 3 to 0.
		Self {
			null: GDTEntry {
				limit_low: 0,
				base_low: 0,
				base_mid: 0,
				access: 0,
				granularity: 0,
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
				limit_low: (mem::size_of::<TSS>() - 1).try_into().unwrap(),
				base_low: (tss >> 0) as u16,
				base_mid: (tss >> 16) as u8,
				access: (0b1_11_0 << 4) | 0x9,
				granularity: 1 << 5,
				base_high: (tss >> 24) as u8,
			},
			tss_extra: GDTEntryHigh {
				base_higher: (tss >> 32) as u32,
				_reserved: 0,
			},
			_marker: PhantomData,
		}
	}
}

impl GDTPointer {
	pub fn new(gdt: Pin<&GDT>) -> Self {
		Self {
			_padding: [0; 3],
			limit: (mem::size_of::<GDT>() - 1).try_into().unwrap(),
			address: gdt.get_ref() as *const _ as u64,
		}
	}

	pub unsafe fn activate(&self) {
		unsafe {
			asm!(
				"lgdt [{ptr}]",
				"mov ax, {kernel_ss}",
				"mov ds, ax",
				"mov es, ax",
				"mov ss, ax",
				// Set TSS
				"mov ax, {tss}",
				"ltr ax",
				kernel_ss = const GDT::KERNEL_SS,
				tss = const GDT::TSS,
				ptr = in(reg) &self.limit
			);
		}
	}
}
