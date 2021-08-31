==========
Interrupts
==========

In general, the kernel handles all interrupts. It then spawns a thread and
passes it to a process.

Interrupts can be reserved by processes. Only one process can reserve an
interrupt at any time.

If IRQs are shared, one process should reserve it and pass the thread to other
processes if necessary.
