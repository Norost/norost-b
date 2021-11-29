use core::mem;

#[macro_export]
macro_rules! __idt_wrap_handler {
	($fn:ident) => {
		{
			const _: fn(u32, *const ()) = $fn;
			#[naked]
			unsafe fn f() {
				asm!("
					pop		rdi		# Error code
					pop		rsi		# RIP
					pop		rax		# CS
					pop		rax		# RFLAGS
					pop		rax		# SS:RSP
					call	{f}
				66:
					jmp		66b		# TODO
				", f = sym $fn, options(noreturn));
			}
			f
		}
	}
}

#[repr(C)]
pub struct IDTEntry {
	offset_low: u16,
	selector: u16,
	ist: u8,
	type_attributes: u8,
	offset_high: u16,
	offset_higher: u32,
	_unused_1: u32,
}

impl IDTEntry {
	const ATTRIBUTE_GATETYPE_INTERRUPT: u8 = 0xe;
	const ATTRIBUTE_GATETYPE_TRAP: u8 = 0xf;
	const ATTRIBUTE_PRESENT: u8 = 0x80;
	const ATTRIBUTE_DPL: u8 = 0x60;

	const EMPTY: Self = Self {
		offset_low: 0,
		selector: 0,
		ist: 0,
		type_attributes: 0,
		offset_high: 0,
		offset_higher: 0,
		_unused_1: 0,
	};

	pub fn new(selector: u16, handler: unsafe fn(), is_trap: bool, ist: u8) -> Self {
		assert!(ist < 8, "ist out of bounds");
		let handler = handler as usize;
		Self {
			offset_low: (handler >> 0) as u16,
			selector,
			ist,
			type_attributes: Self::ATTRIBUTE_PRESENT
				| Self::ATTRIBUTE_DPL
				| is_trap
					.then(|| Self::ATTRIBUTE_GATETYPE_TRAP)
					.unwrap_or(Self::ATTRIBUTE_GATETYPE_INTERRUPT),
			offset_high: (handler >> 16) as u16,
			offset_higher: (handler >> 32) as u32,
			_unused_1: 0,
		}
	}
}

#[repr(C)]
pub struct IDT<const L: usize> {
	descriptors: [IDTEntry; L],
}

impl<const L: usize> IDT<L> {
	pub const fn new() -> Self {
		Self {
			descriptors: [IDTEntry::EMPTY; L],
		}
	}

	pub fn set(&mut self, index: usize, entry: IDTEntry) {
		self.descriptors[index] = entry;
	}
}

#[repr(C)]
#[repr(packed)]
pub struct IDTPointer {
	limit: u16,
	offset: u64,
}

impl IDTPointer {
	pub fn new<const L: usize>(idt: &'static IDT<L>) -> Self {
		Self {
			limit: u16::try_from(mem::size_of_val(idt) - 1).unwrap(),
			offset: idt as *const _ as u64,
		}
	}

	pub fn activate(&self) {
		unsafe {
			asm!("lidt [{0}]", in(reg) self);
		}
	}
}
