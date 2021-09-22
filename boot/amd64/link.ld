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
		*(.init .init.*)
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

	boot_top = .;
}
