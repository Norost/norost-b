.intel_syntax noprefix

.globl		_start

.equ		IDENTITY_MAP_ADDRESS, 0xffffc00000000000

# .init is placed at the start of the executable, which is convenient when
# identity-mapping it.
.section	.init
_start:
	# Disable interrupts
	cli

	# Set up stack
	lea		esp, [stack_top]
	mov		ebp, esp

	sub		esp, 12		# Reserve space for u64 returned value and u32 PML4 address
	push	esp
	mov		ecx, eax	# multiboot2 magic value
	mov		edx, ebx	# multiboot2 information structure

	call	main

	# Use ebp/esp as they become useless after enabling paging anyways
	mov		edi, dword ptr [esp + 12]	# Load info
	mov		eax, dword ptr [esp +  8]	# Load PML4
	mov		ebp, dword ptr [esp +  4]	# Load entry point (high)
	mov		esp, dword ptr [esp +  0]	# Load entry point (low)

	# Enable PAE
	mov		ecx, cr4
	or		ecx, 0x20
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
	movabs	rax, IDENTITY_MAP_ADDRESS
	or		rdi, rax

	# Jump to kernel entry
	jmp		rsp


.section	.bss.stack
stack_bottom:
	.zero	0x1000
stack_top:
