.intel_syntax	noprefix

.globl			_start
.globl			_stack_top

.section .text
_start:
	lea		rsp, [rip + _stack_top]
	jmp		main

.section .bss.stack
_stack_bottom:
	.zero	0x1000 * 4
_stack_top:
