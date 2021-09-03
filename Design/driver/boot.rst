====
Boot
====

On boot, the kernel spawns one initial process. This process has full
privileges and is able to map any memory region directly into its address
space.

Unlike other processes, it must set up its own stack. It is also unable to take
advantage of special ``.norost`` sections.

This process is expected to spawn the drivers for all the devices on the
system. Before spawning any process it should first check if the kernel has a
built-in driver for any device however.
