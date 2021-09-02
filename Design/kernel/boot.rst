====
Boot
====

Boot has three stages:

* Bootloader
* Early boot
* Loading drivers
* Loading init (PID 1)


Bootloader
~~~~~~~~~~

The exact mechanism of the bootloader is implementation-dependent, but it must
always do the following:

#. If necessary, ensure only one hart is running and the others halt.
#. Identity map the lower half of memory.
#. Properly map the kerne.l
#. Pass any memory region in a0 (pointer) and a1 (size).
   * Preferably, this is the largest region found.
#. Pass the pointer and size to the kernel start and end address in physical
   memory.
#. Pass the pointer and size of the initramfs start and end in physical memory.
#. Pass the pointer and size of the DTB / ACPI / ... start and end in physical
   memory.
#. Jump to kernel entry point.


Early boot
~~~~~~~~~~

#. The memory allocator is set up with whatever initial size of page is
   appropriate.
   * This defaults to 1 MB. Other sizes need to be configured at compile time.
#. The page frame allocator is set up.
   * The structures use memory from the dynamic allocator, hence why the
   allocator was set up first.
   * Using the memory allocator allows use of a hugepage without wasting
   memory, improving overall efficiency.
#. Create init process.
#. Wake up all harts and enter executor loop.


Boot
~~~~

#. Load drivers
#. Start init


Init
~~~~

#. Do whatever
