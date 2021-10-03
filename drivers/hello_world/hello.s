.intel_syntax noprefix

.globl _start

.section .text
_start:
.l0:
	xor		eax, eax
	lea		rdi, [hello]
	mov		rsi, hello_end - hello
	syscall
	mov		ecx, 2000 * 1000 * 1000
.l1:
	loop	.l1
	jmp		.l0

.section .rodata
hello:
	.ascii	"Hello, world!"
hello_end:
