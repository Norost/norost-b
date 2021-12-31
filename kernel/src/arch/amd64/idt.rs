use core::arch::asm;
use core::mem;

#[macro_export]
macro_rules! __idt_wrap_handler {
	(trap $fn:ident) => {
		{
			const _: fn(u32, *const ()) = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				asm!("
					pop		rdi		# Error code
					pop		rsi		# RIP

					cld
					call	{f}

					rex64 iretq
				", f = sym $fn, options(noreturn));
			}
			$crate::arch::amd64::idt::Handler::Trap(f)
		}
	};
	(int $fn:ident) => {
		{
			const _: fn(*const ()) = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				asm!("
					# Save thread state
					push	rax
					push	rbx
					push	rcx
					push	rdx
					push	rdi
					push	rsi
					push	rbp
					push	r8
					push	r9
					push	r10
					push	r11
					push	r12
					push	r13
					push	r14
					push	r15
					mov		gs:[8], rsp		# Save kernel stack pointer

					# Call handler
					mov		rdi, [rsp + 15 * 8]		# RIP
					cld
					call	{f}

					# Mark EOI
					mov		ecx, {msr}
					rdmsr
					and		eax, 0xfffff000
					movabs	rcx, {virt_ident}
					or		rax, rcx
					mov		DWORD PTR [rax + {eoi}], 0

					# Restore thread state
					pop		r15
					pop		r14
					pop		r13
					pop		r12
					pop		r11
					pop		r10
					pop		r9
					pop		r8
					pop		rbp
					pop		rsi
					pop		rdi
					pop		rdx
					pop		rcx
					pop		rbx
					pop		rax

					iretq
				", f = sym $fn,
				msr = const super::msr::IA32_APIC_BASE_MSR,
				eoi = const 0xb0,
				virt_ident = const 0xffff_c000_0000_0000u64,
				options(noreturn));
			}
			$crate::arch::amd64::idt::Handler::Int(f)
		}
	};
	(int noreturn $fn:ident) => {
		{
			const _: fn(*const ()) -> ! = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				asm!("
					# Save thread state
					push	rax
					push	rbx
					push	rcx
					push	rdx
					push	rdi
					push	rsi
					push	rbp
					push	r8
					push	r9
					push	r10
					push	r11
					push	r12
					push	r13
					push	r14
					push	r15
					mov		gs:[8], rsp		# Save kernel stack pointer

					# Mark EOI
					mov		ecx, {msr}
					rdmsr
					and		eax, 0xfffff000
					movabs	rcx, {virt_ident}
					or		rax, rcx
					mov		DWORD PTR [rax + {eoi}], 0

					# Call handler
					mov		rdi, [rsp + 15 * 8]		# RIP
					cld
					jmp		{f}
				", f = sym $fn,
				msr = const super::msr::IA32_APIC_BASE_MSR,
				eoi = const 0xb0,
				virt_ident = const 0xffff_c000_0000_0000u64,
				options(noreturn));
			}
			$crate::arch::amd64::idt::Handler::Int(f)
		}
	};
}

#[derive(Clone, Copy)]
pub enum Handler {
	Int(unsafe extern "C" fn()),
	Trap(unsafe extern "C" fn()),
}

#[naked]
unsafe extern "C" fn irq_noop() {
	asm!("
		push	rax
		movabs	rax, {eoi}
		mov		DWORD PTR [rax], 0
		pop		rax
		iretq
		",
		eoi = const 0xffff_c000_fee0_00b0u64,
		options(noreturn),
	);
}

pub const NOOP: Handler = Handler::Int(irq_noop);

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

	pub fn new(selector: u16, handler: Handler, ist: u8) -> Self {
		assert!(ist < 8, "ist out of bounds");
		let (handler, is_trap) = match handler {
			Handler::Int(h) => (h as usize, false),
			Handler::Trap(h) => (h as usize, true),
		};
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
