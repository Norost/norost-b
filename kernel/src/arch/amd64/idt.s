# Generate 223 IRQ stubs which each push the IRQ number.
#
# This approach is used because it's very useful to avoid duplicating (machine) code for handlers
# that are shared between IRQs.
#
# We only generate 256 - 33 = 223 stubs because the first 33 IRQs have a well-defined purpose.
# It's only the dynamically assigned interrupts that are muddy.

.intel_syntax noprefix

.section .text.idt

.equ IRQ_STUB_OFFSET, 33

irq_stub_table:
.rept 256 - IRQ_STUB_OFFSET
	call irq_handler
.endr

irq_handler:
	# IRQs should not be using CPU local data, so ignore for now at least.
	# If we do end up needing CPU local data in an IRQ, slap me for being short-sighted.

	# Save scratch registers
	# except rax, see below
	push rdi
	push rsi
	push rdx
	push rcx
	push r8
	push r9
	push r10
	push r11

	# xchg has an implicit lock, so it's horrendously slow.
	# Still, we can emulate it efficiently with a scratch register, which we'll need as
	# argument register anyways :)
	mov rdi, [rsp + 8 * 8] # load caller *next* rip
	mov [rsp + 8 * 8], rax # store rax

	# handler base (put it here to take advantage of pipelining)
	lea rax, [rip + irq_handler_table]

	# offset in handler table is (rip - 5 - irq_stub_table) / 5 = (rip - irq_stub_table) / 5 - 1
	lea rcx, [rip + irq_stub_table]
	sub rdi, rcx
	# The trick here is to find some large enough power-of-two divisor, then find the corresponding
	# dividend to approach 1/5, i.e. divisor / 5 = dividend.
	# Thanks GCC!
	imul edi, edi, 205
	shr edi, 10

	# handler address
	mov rax, [rax + rdi * 8 - 8]

	# argument
	add edi, IRQ_STUB_OFFSET - 1

	# Call handler
	cld
	call rax

	# TODO try to send EOI here, saves a few lines of code (and instructions)

	# Restore thread state
	pop r11
	pop r10
	pop r9
	pop r8
	pop rcx
	pop rdx
	pop rsi
	pop rdi
	pop rax

	iretq
