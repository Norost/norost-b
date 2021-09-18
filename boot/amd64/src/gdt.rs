use core::mem;

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
pub struct GDT {
	null: GDTEntry,
	code: GDTEntry,
	data: GDTEntry,
}

impl GDT {
	pub const fn new() -> Self {
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
				access: 0b1_00_1_1_0_1_0,
				granularity: (1 << 7) | (1 << 5) | 0xf,
				base_high: 0,
			},
			data: GDTEntry {
				limit_low: 0xffff,
				base_low: 0,
				base_mid: 0,
				access: 0b1_00_1_0_0_1_0,
				granularity: (1 << 7) | (1 << 5) | 0xf,
				base_high: 0,
			},
		}
	}
}

#[repr(C)]
pub struct GDTPointer<'a> {
	_padding: [u16; 1],
	limit: u16,
	address: &'a GDT,
}

impl<'a> GDTPointer<'a> {
	pub const fn new(gdt: &'a GDT) -> Self {
		Self {
			_padding: [0; 1],
			limit: (mem::size_of::<GDT>() - 1) as u16,
			address: gdt,
		}
	}

	pub unsafe fn activate(&self) {
		asm!("lgdtl ({0})", in(reg) &self.limit, options(att_syntax));
	}
}
