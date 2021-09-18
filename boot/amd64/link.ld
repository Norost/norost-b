ENTRY(_start)

SECTIONS
{
	. = 1M;

	boot_bottom = .;

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

	boot_top = .;

	/DISCARD/ :
	{
		*(.note .note.*)
	}
}
