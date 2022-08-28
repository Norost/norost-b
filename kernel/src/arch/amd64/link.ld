ENTRY(_start)

SECTIONS
{
	. = 0xffff800000000000;
 
	.text :
	{
		KEEP(*(.text.idt))
		*(.text .text.*)
	}
 
	.rodata :
	{
		*(.rodata .rodata.*)
	}

	. = ALIGN(4K);
 
	.data :
	{
		*(.data .data.*)
	}

	/* FIXME this shouldn't be necessary */
	. = ALIGN(4K);
 
	.bss :
	{
		*(COMMON)
		*(.bss .bss.*)
	}

	. = 0xffff800000000000 | (1 << 21) - (4096 * 1);

	.vsyscall.data :
	{
		*(.vsyscall.data.timeinfo)
	}

	/DISCARD/ :
	{
		*(.note .note.*)
	}
}
