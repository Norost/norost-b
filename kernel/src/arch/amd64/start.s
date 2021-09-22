.intel_syntax	noprefix

.globl			_start

.equ		IDENTITY_MAP_ADDRESS, 0xffffc00000000000

.section .text
_start:
	lea		rsp, [rip + _stack_top]
	mov		rbp, rsp
	mov		rax, IDENTITY_MAP_ADDRESS
	add		rdi, rax
	jmp		main

.section .bss.stack
_stack_bottom:
	.zero	0x1000
_stack_top:
