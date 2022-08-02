use core::{arch::asm, mem};

pub macro __swap_gs() {
	"cmp DWORD PTR [rsp + 8], 8",
	"jz 2f",
	"swapgs",
	"2:",
}

#[macro_export]
macro_rules! __idt_wrap_handler {
	($fn:path) => {
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
			f
		}
	};
	(rip $fn:path) => {
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

						"mov rsi, [rsp + 9 * 8]", // RIP
						"cld",
						"call {f}",

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
						f = sym $fn, options(noreturn)
					);
				}
			}
			f
		}
	};
	(error rip $fn:path) => {
		{
			const _: extern "C" fn(u32, *const ()) = $fn;
			#[naked]
			unsafe extern "C" fn f() {
				unsafe {
					core::arch::asm!(
						// Check if we need to swapgs by checking $cl
						"cmp DWORD PTR [rsp + 8], 8",
						"jz 2f",
						"swapgs",
						"2:",

						"xchg rdi, [rsp]", // Error code

						// Save scratch registers
						// Note that we already saved $rdi
						"push rax",
						"push rcx",
						"push rdx",
						"push rsi",
						"push r8",
						"push r9",
						"push r10",
						"push r11",

						"mov rsi, [rsp + 9 * 8]", // RIP
						"cld",
						"call {f}",

						"pop r11",
						"pop r10",
						"pop r9",
						"pop r8",
						"pop rsi",
						"pop rdx",
						"pop rcx",
						"pop rax",
						"pop rdi",

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
			f
		}
	};
	(nmi $fn:path) => {
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
			f
		}
	};
	(noreturn savethread $fn:path) => {
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
			f
		}
	};
}

#[macro_export]
macro_rules! wrap_idt {
	(@INTERNAL [$($type:ident)*] $f:path [$ist:literal]) => {
		$crate::arch::amd64::idt::IDTEntry::new(
			1 * 8,
			$crate::__idt_wrap_handler!($($type)* $f),
			$ist,
		)
	};
	($f:path) => { $crate::wrap_idt!(@INTERNAL [] $f [0]) };
	(rip $f:path) => { $crate::wrap_idt!(@INTERNAL [rip] $f [0]) };
	(error rip $f:path) => { $crate::wrap_idt!(@INTERNAL [error rip] $f [0]) };
	(error rip $f:path [$ist:literal]) => { $crate::wrap_idt!(@INTERNAL [error rip] $f [$ist]) };
	(nmi $f:path) => { $crate::wrap_idt!(@INTERNAL [nmi] $f [0]) };
	(noreturn $f:path) => { $crate::wrap_idt!(@INTERNAL [noreturn] $f [0]) };
	(noreturn savethread $f:path) => {
		$crate::wrap_idt!(@INTERNAL [noreturn savethread] $f [0])
	};
}

pub type Handler = unsafe extern "C" fn();

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
	/// Disables interrupts on ISR call. This has nothing to do with the actual type of interrupt
	/// or exception.
	const ATTRIBUTE_GATETYPE_INTERRUPT: u8 = 0xe;
	/// Keep interrupts enabled on ISR call. This has nothing to do with the actual type of
	/// interrupt or exception.
	#[allow(dead_code)]
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
		Self::new_raw(selector, handler as _, ist)
	}

	const fn new_raw(selector: u16, handler: u64, ist: u8) -> Self {
		assert!(ist < 8, "ist out of bounds");
		Self {
			offset_low: (handler >> 0) as u16,
			selector,
			ist,
			type_attributes: Self::ATTRIBUTE_PRESENT
				| Self::ATTRIBUTE_DPL
				| Self::ATTRIBUTE_GATETYPE_INTERRUPT,
			offset_high: (handler >> 16) as u16,
			offset_higher: (handler >> 32) as u32,
			_unused_1: 0,
		}
	}
}

#[repr(C)]
pub struct IDT {
	descriptors: [IDTEntry; 256],
}

impl IDT {
	pub const fn new() -> Self {
		let mut descriptors = [IDTEntry::EMPTY; 256];
		let mut offset = 0;
		let mut i = super::IRQ_STUB_OFFSET;
		while i < descriptors.len() {
			descriptors[i] = IDTEntry::new_raw(1 * 8, super::KERNEL_BASE + offset, 0);
			offset += 5;
			i += 1;
		}
		Self { descriptors }
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
	pub fn new(idt: &'static IDT) -> Self {
		Self {
			limit: u16::try_from(mem::size_of_val(idt) - 1).unwrap(),
			offset: idt as *const _ as u64,
		}
	}

	pub fn activate(&self) {
		unsafe {
			asm!("lidt [{0}]", in(reg) self, options(readonly, nostack));
		}
	}
}
