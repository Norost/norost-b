.intel_syntax	noprefix

.globl _start
.globl _stack_top

.section .text.start
_start:
	lea rsp, [rip + _stack_top]
	jmp main

.section .bss.stack
.align 16
_stack_bottom:
.zero 0x1000 * 4 - 8
_stack_top:
.zero 8
