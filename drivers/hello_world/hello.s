.intel_syntax noprefix

.equ	SYS_SYSLOG, 0
.equ	SYS_OPEN, 8
.equ	SYS_SLEEP, 10
.equ	SYS_WRITE, 12

.globl _start

.section .text

_start:

	lea		rsp, [rip + stack_end]

	jmp		3f

3:
	mov		eax, SYS_SYSLOG
	lea		rdi, [hello]			# address of string
	mov		rsi, hello_end - hello	# length of string
	syscall
	
	# Open object
	mov		eax, SYS_OPEN
	mov		rdi, 1			# table
	mov		rsi, 0			# object
	syscall
	test	eax, eax
	jnz		panic
	mov		r15, rdx

4:
	# Write to object
	mov		eax, SYS_WRITE
	mov		rdi, r15				# handle
	lea		rsi, [rip + hello]		# base pointer
	mov		rdx, hello_end - hello	# length
	mov		rcx, 0					# offset
	syscall
	test	eax, eax
	jnz		panic

	# Sleep for 1 second (1_000_000 µs)
	mov		rdi, 1000000
	call	sleep

	jmp		4b

panic:
	mov		eax, SYS_SYSLOG
	lea		rdi, [panic_msg]
	mov		rsi, panic_msg_end - panic_msg
	syscall
2:
	mov		rdi, -1
	call	sleep
	jmp		2b


sleep:
	mov		eax, SYS_SLEEP
	syscall
	test	eax, eax
	jnz		panic
	ret


.section .rodata
hello:
	.ascii	"Hello, world!"
hello_end:

client_queue_test:
	.ascii	"Hello from the client queue!"
client_queue_test_end:

panic_msg:
	.ascii	"Panic!"
panic_msg_end:

.section .bss
.p2align	3
stack:
	.zero 64
stack_end:

.p2align	3
client_queue:
	.zero 8
