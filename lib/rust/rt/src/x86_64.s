.globl _start
.globl memcpy
.globl memmove
.globl memset
.globl memcmp

.equ ID_EXIT, 16

# SysV ABI:
# - Parameter: rdi, rsi, rdx, ...
# - Return: rax, rdx
# - Scratch: rcx, ...
# - DF is cleared by default

.section .bss._stack
.p2align 12
# align stack beforehand so we can save on a push
	.zero (1 << 17) - 8
_stack:
	.zero 8

.section .text._start
# rax: thread handle
# rsp: pointer to program arguments & environment variables
# rsp can also be used as stack but meh
_start:
	mov rdi, rsp
	lea rsp, [rip + _stack]
	call main
	mov edi, eax
	mov eax, ID_EXIT
	syscall
	ud2 # Just in case something went horribly wrong

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

.section .text.memcmp
.p2align 4
# int memset(void *a, void *b, size_t n)
# rep cmpsb is very slow, so implement something manually
#  - benchmark on 2G data: 65ms manual version vs 810ms rep cmpsb
memcmp:
	mov r8, rdx
	and r8, ~0x7
	add r8, rdi

	add rdx, rdi

	# make equal so if n == 0 then eax - ecx == 0 too
	mov eax, ecx

	# Compare in chunks of 8 bytes
	jmp 3f
2:
	mov rax, QWORD PTR [rdi]
	# if non-zero, one of the bytes differs
	# don't increase rdi/rsi & rescan with byte loads
	cmp rax, QWORD PTR [rsi]
	jne 4f
	# see above
	add rdi, 8
	add rsi, 8
3:
	cmp rdi, r8
	jnz 2b
4:

	# Compare individual bytes
	jmp 3f
2:
	movsx eax, BYTE PTR [rdi]
	movsx ecx, BYTE PTR [rsi]
	cmp eax, ecx
	jne 4f
	inc rdi
	inc rsi
3:
	cmp rdi, rdx
	jnz 2b
4:

	sub eax, ecx
	ret
