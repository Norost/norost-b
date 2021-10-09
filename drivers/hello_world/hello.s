.intel_syntax noprefix

.globl _start

.section .text

_start:

	lea		rsp, [rip + stack_end]
	
	call	new_client_queue

	lea		rdi, [rip + client_queue_test]
	mov		rsi, client_queue_test_end - client_queue_test
	mov		edx, 1337
	call	submit_client_queue_entry

.l0:
	xor		eax, eax				# syslog
	lea		rdi, [hello]			# address of string
	mov		rsi, hello_end - hello	# length of string
	syscall

	mov		ecx, 2000 * 1000 * 1000 # idle for 2G cycles
.l1:
	loop	.l1

	call	push_client_queue
	jmp		.l0


new_client_queue:
	mov		eax, 1					# init_client_queue
	mov		edi, 0x123000			# address to map it to
	mov		esi, 4					# 16 submission entries
	mov		edx, 5					# 32 completion entries
	syscall
	test	eax, eax
	jnz		panic
	mov		[rip + client_queue], rdi
	ret


push_client_queue:
	mov		eax, 2					# push_client_queue
	syscall
	test	eax, eax
	jnz		panic
	ret


submit_client_queue_entry:
	push	rbx

	mov		rax, [rip + client_queue]
	mov		ebx, [rax + 4 * 2]
	mov		ecx, [rax + 4 * 0]
	
	and		ebx, ecx
	shl		ebx, 5
	lea		r8, [rax + 4 * 6 + rbx]
	mov		bl, 127
	mov		[r8 +  0], bl			# OP_SYSLOG
	mov		[r8 +  8], rdi
	mov		[r8 + 16], rsi
	mov		[r8 + 56], rdx			# user_data

	# Update submission head
	inc		ecx
	mov		[rax + 0], ecx

	pop		rbx
	ret


panic:
	jmp		panic


.section .rodata
hello:
	.ascii	"Hello, world!"
hello_end:
client_queue_test:
	.ascii	"Hello from the client queue!"
client_queue_test_end:

.section .bss
.p2align	3
stack:
	.zero 64
stack_end:

.p2align	3
client_queue:
	.zero 8
