.intel_syntax noprefix
.weak _start
.weak memcpy
.weak memmove
.weak memset

# SysV ABI:
# - Parameter: rdi, rsi, rdx, ...
# - Return: rax, rdx
# - Scratch: rcx, ...
# - DF is cleared by default

.section .bss
.p2align 12
	.zero 4096
	.zero 4096
	.zero 4096
	.zero 4096
_stack:

.section .text._start
_start:
	lea rsp, [_stack + 8]
	call main

	# exit
	# Not actually supported so we go KABOOM instead
2:
	hlt
	# Just in case...
	jmp 2b

.section .text.memcpy
.p2align 4
# void *memcpy(void *dest, void *src, size_t n)
memcpy:
	mov rax, rdi
	mov rcx, rdx
	rep movsb
	ret

.section .text.memmove
.p2align 4
# void *memmove(void *dest, void *src, size_t n)
memmove:
	mov rax, rdi
	mov rcx, rdx
	cmp rsi, rdi
	jl	2f
	# rsi > rdi -> copy lowest first
	rep movsb
	ret
2:
	# rsi < rdi -> copy highest first
	std
	add rsi, rcx
	add rdi, rcx
	dec rsi
	dec rdi
	rep movsb
	cld
	ret

.section .text.memmove
.p2align 4
# void *memset(void *dest, int c, size_t n)
memset:
	mov rcx, rdx
	xchg rax, rsi
	rep stosb
	mov rax, rsi
	ret
