.intel_syntax noprefix

.globl _start

.section .text

_start:

	lea		rsp, [rip + stack_end]
	
	call	new_client_queue

.l0:
	xor		eax, eax				# syslog
	lea		rdi, [hello]			# address of string
	mov		rsi, hello_end - hello	# length of string
	syscall

	mov		ecx, 2000 * 1000 * 1000 # idle for 2G cycles
.l1:
	loop	.l1

	jmp		.l0


new_client_queue:
	mov		eax, 1					# init_client_queue
	mov		edi, 0x123000			# address to map it to
	mov		esi, 4					# 16 submission entries
	mov		edx, 5					# 32 completion entries
	syscall
	ret

.section .rodata
hello:
	.ascii	"Hello, world!"
hello_end:

.section .bss
.p2align	3
stack:
	.zero 64
stack_end:
