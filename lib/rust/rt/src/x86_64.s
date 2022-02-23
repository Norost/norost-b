.weak _start
.weak memcpy
.weak memmove
.weak memset

# SysV ABI:
# - Parameter: rdi, rsi, rdx, ...
# - Return: rax, rdx
# - Scratch: rcx, ...
# - DF is cleared by default

.section .bss._stack
.p2align 12
# align stack beforehand so we can save on a push
	.zero (1 << 16) - 8
_stack:
	.zero 8

.section .text._start
_start:
	lea rsp, [rip + _stack]
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
	lea rsi, [rsi + rcx - 1]
	lea rdi, [rdi + rcx - 1]
	rep movsb
	cld
	ret

.section .text.memset
.p2align 4
# void *memset(void *dest, int c, size_t n)
memset:
	mov rcx, rdx
	xchg rax, rsi
	rep stosb
	mov rax, rsi
	ret
