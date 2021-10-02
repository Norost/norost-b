.intel_syntax noprefix

.globl _start

.section .text
_start:
.l0:
	syscall
	mov		ecx, 2000 * 1000 * 1000
.l1:
	loop	.l1
	jmp		.l0
