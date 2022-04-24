# Simple bootloader for microkernels

## Features

- Switch from protected to long mode.
- Identity-mapping all memory regions as given by the previous boot stage
- Loading a kernel in ELF format.
- Loading drivers / init programs with arguments.
- Can be used by multiboot2-compatible bootloaders (e.g. GRUB).
- Provide boot information to next stage in a simple format.

## How it works.

All information is put in a page-aligned 64KB buffer and a pointer to the start of this buffer
is passed to the next stage in the RDI register.
