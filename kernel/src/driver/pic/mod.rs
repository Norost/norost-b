pub(super) fn init() {
	unsafe {
		asm!("
			## Setup PIC
			# ICW1 (allow ICW4 & set PIC to be initialized)
			mov		al, 0x11
			out		0x20, al
			out		0xa0, al
			# ICW2 (map IVT)
			mov		al, 0x20
			out		0x21, al
			mov		al, 0x28
			out		0xa1, al
			# ICW3
			mov		al, 0x4 # 100 # Slave PIC is at IRQ2
			out		0x21, al
			mov		al, 0x2 # 010 # No-op I guess?
			out		0xa1, al
			# ICW4 (set 80x86 mode)
			mov		al, 0x1
			out		0x21, al
			out		0xa1, al
			# OCW1 (unmask IRQ 8)
			mov		al, 0xff
			#mov		al, 0xfb
			out		0x21, al
			#mov		al, 0xfe
			out		0xa1, al
			",
			lateout("al") _
		);
	}
}
