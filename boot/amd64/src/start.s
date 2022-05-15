.intel_syntax noprefix

.globl		_start

.equ		IDENTITY_MAP_ADDRESS, 0xffffc00000000000

# .init is placed at the start of the executable, which is convenient when
# identity-mapping it.
.section	.init
_start:
	# Set up stack
	lea		esp, [stack_top]
	mov		ebp, esp

	sub		esp, 16		# Reserve space for u64 returned value and u32 PML4 address
	push	esp
	mov		ecx, eax	# multiboot2 magic value
	mov		edx, ebx	# multiboot2 information structure

	call	main

	# Use ebp/esp as they become useless after enabling paging anyways
	mov		edi, dword ptr [esp + 12]	# Load info
	mov		eax, dword ptr [esp +  8]	# Load PML4
	mov		ebp, dword ptr [esp +  4]	# Load entry point (high)
	mov		esp, dword ptr [esp +  0]	# Load entry point (low)

	# Enable PAE & PGE
	mov		ecx, cr4
	or		ecx, (1 << 5) | (1 << 4)
	mov		cr4, ecx

	# Set PML4
	mov		cr3, eax

	# Enable long mode
	mov		ecx, 0xc0000080	# IA32_EFER
	rdmsr
	or		eax, 0x100		# Enable long mode
	wrmsr

	# Enable paging
	mov		eax, cr0
	or		eax, 0x80000000
	mov		cr0, eax

	# Switch to long mode
	ljmp	0x8, realm64

.code64
realm64:

	# Fix entry address
	mov		cl, 32
	shlq	rbp, cl
	orq		rsp, rbp

	# Setup data segment properly
	mov		ax, 0x10
	mov		ds, ax
	mov		es, ax
	mov		fs, ax
	mov		gs, ax
	mov		ss, ax

	# Fix pointer to boot info structure to point to identity-mapped space
	movabs	rbx, IDENTITY_MAP_ADDRESS
	or		rdi, rbx

	# Jump to identity-mapped space so we can unmap the last page not in the higher half.
	lea		rax, [rip + 2f]
	or		rax, rbx
	jmp		rax

2:
	# Fix the GDT to point to the higher-half identity-mapped space.
	lgdt	[rip + _gdt_ptr64]

	# Unmap the last page. We can do this the lazy way by simply zeroing out the
	# lower half of the table.
	# TODO we leak two pages by doing this.
	mov		rax, cr3
	or		rax, rbx
	mov		rbx, rdi
	mov		rdi, rax
	mov		rcx, 2048
	rep stosb
	mov		rdi, rbx

	# Reload PML4 to ensure the unmapped pages are flushed
	mov rax, cr3
	mov cr3, rax

	# Jump to kernel entry
	jmp		rsp

_gdt_ptr64:
	.word	8 * 3 - 1
	.long	_gdt
	.long	0xffffc000

.section	.bss.stack
	.p2align 2
stack_bottom:
	.zero	0x1000 - 8 # Those 8 bytes over the page border keep me up at night
stack_top:
