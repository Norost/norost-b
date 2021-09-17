use core::convert::TryInto;
use core::mem;
use core::pin::Pin;

#[repr(C)]
struct GDTEntry {
	limit_low: u16,
	base_low: u16,
	base_mid: u8,
	access: u8,
	granularity: u8,
	base_high: u8,
}

#[repr(C)]
pub struct GDT64 {
	null: GDTEntry,
	code: GDTEntry,
	data: GDTEntry,
}

impl GDT64 {
	pub const fn new() -> Self {
		// Values copied from https://wiki.osdev.org/index.php?title=Setting_Up_Long_Mode&oldid=22154#Entering_the_64-bit_Submode
		Self {
			null: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0,
				granularity: 1,
				base_high: 0,
			},
			code: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1001_1010,
				granularity: 0b1010_1111,
				base_high: 0,
			},
			data: GDTEntry {
				limit_low: 0x0,
				base_low: 0,
				base_mid: 0,
				access: 0b1001_0010,
				granularity: 0b0000_00000,
				base_high: 0,
			},
		}
	}
}

#[repr(C)]
#[repr(packed)]
pub struct GDT64Pointer {
	limit: u16,
	address: u32,
}

impl GDT64Pointer {
	pub fn new(gdt: Pin<&GDT64>) -> Self {
		unsafe {
			Self {
				limit: mem::size_of::<GDT64>().try_into().unwrap(),
				address: gdt.get_ref() as *const _ as u32,
			}
		}
	}

	pub unsafe fn activate(&self) {
		asm!("lgdtl ({0})", in(reg) self as *const _, options(att_syntax));
	}
}
