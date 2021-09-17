ENTRY(_start)

SECTIONS
{
	. = 1M;

	.multiboot :
	{
		KEEP(*(.multiboot));
	}

	.text :
	{
		*(.text .text.*)
	}
 
	.rodata :
	{
		*(.rodata .rodata.*)
	}
 
	.data :
	{
		*(.data .data.*)
	}
 
	.bss :
	{
		*(COMMON)
		*(.bss .bss.*)
	}

	/DISCARD/ :
	{
		*(.note .note.*)
	}
}
