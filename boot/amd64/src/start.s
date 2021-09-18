.globl		_start
.globl		stack_top
.globl		stack_bottom

.section	.text
_start:
	cli
	lea		stack_top, %esp
	mov		%esp, %ebp
	push	%ebx
	push	%eax
	call	main

.section	.bss
.balign		8
stack_bottom:
	.zero	0x1000
stack_top:
