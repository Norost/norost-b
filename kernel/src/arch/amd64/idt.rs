use core::arch::asm;
use core::mem;

#[macro_export]
macro_rules! __idt_wrap_handler {
	(trap $fn:path) => {
		{
			const _: extern "C" fn(*const ()) = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"pop rsi", // RIP
						"push rsi",
						"cld",
						"call {f}",

						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"iretq",
						f = sym $fn, options(noreturn)
					);
				}
			}
			$crate::arch::amd64::idt::Handler::Trap(f)
		}
	};
	(trap error $fn:path) => {
		{
			const _: extern "C" fn(u32, *const ()) = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						"pop rdi", // Error code
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"pop rsi", // RIP
						"push rsi",
						"cld",
						"call {f}",

						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"iretq",
						f = sym $fn, options(noreturn)
					);
				}
			}
			$crate::arch::amd64::idt::Handler::Trap(f)
		}
	};
	(int $fn:path) => {
		{
			const _: extern "C" fn() = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						// Save scratch registers
						"push rax",
						"push rcx",
						"push rdx",
						"push rdi",
						"push rsi",
						"push r8",
						"push r9",
						"push r10",
						"push r11",

						// Call handler
						"cld",
						"call {f}",

						// Restore thread state
						"pop r11",
						"pop r10",
						"pop r9",
						"pop r8",
						"pop rsi",
						"pop rdi",
						"pop rdx",
						"pop rcx",
						"pop rax",

						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"iretq",
						f = sym $fn,
						options(noreturn)
					);
				}
			}
			$crate::arch::amd64::idt::Handler::Int(f)
		}
	};
	(int nmi $fn:path) => {
		{
			const _: extern "C" fn() = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						// FIXME we need to use RDMSR to ensure swapgs has been executed.
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						// Save scratch registers
						"push rax",
						"push rcx",
						"push rdx",
						"push rdi",
						"push rsi",
						"push r8",
						"push r9",
						"push r10",
						"push r11",

						// Call handler
						"cld",
						"call {f}",

						// Restore thread state
						"pop r11",
						"pop r10",
						"pop r9",
						"pop r8",
						"pop rsi",
						"pop rdi",
						"pop rdx",
						"pop rcx",
						"pop rax",

						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"iretq",
						f = sym $fn,
						options(noreturn)
					);
				}
			}
			$crate::arch::amd64::idt::Handler::Int(f)
		}
	};
	(int noreturn savethread $fn:path) => {
		{
			const _: extern "C" fn() -> ! = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						// Save full thread state
						"push rax",
						"push rbx",
						"push rcx",
						"push rdx",
						"push rdi",
						"push rsi",
						"push rbp",
						"push r8",
						"push r9",
						"push r10",
						"push r11",
						"push r12",
						"push r13",
						"push r14",
						"push r15",
						// Save kernel stack pointer
						"mov gs:[{kernel_stack_ptr}], rsp",

						// Call handler
						"cld",
						"jmp {f}",
						f = sym $fn,
						kernel_stack_ptr =
							const $crate::arch::amd64::syscall::CpuData::KERNEL_STACK_PTR,
						options(noreturn)
					);
				}
			}
			$crate::arch::amd64::idt::Handler::Int(f)
		}
	};
}

#[macro_export]
macro_rules! wrap_idt {
	(@INTERNAL [$($type:ident)+] $f:path [$ist:literal]) => {
		$crate::arch::amd64::idt::IDTEntry::new(
			1 * 8,
			$crate::__idt_wrap_handler!($($type)+ $f),
			$ist,
		)
	};
	(trap $f:path) => { $crate::wrap_idt!(@INTERNAL [trap] $f [0]) };
	(trap error $f:path) => { $crate::wrap_idt!(@INTERNAL [trap error] $f [0]) };
	(trap error $f:path [$ist:literal]) => { $crate::wrap_idt!(@INTERNAL [trap error] $f [$ist]) };
	(int $f:path) => { $crate::wrap_idt!(@INTERNAL [int] $f [0]) };
	(int nmi $f:path) => { $crate::wrap_idt!(@INTERNAL [int nmi] $f [0]) };
	(int noreturn $f:path) => { $crate::wrap_idt!(@INTERNAL [int noreturn] $f [0]) };
	(int noreturn savethread $f:path) => {
		$crate::wrap_idt!(@INTERNAL [int noreturn savethread] $f [0])
	};
}

#[derive(Clone, Copy)]
pub enum Handler {
	Int(unsafe extern "C" fn()),
	Trap(unsafe extern "C" fn()),
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
	const ATTRIBUTE_DPL: u8 = 0x00;

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
