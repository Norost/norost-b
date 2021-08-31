=======
Process
=======

A process is an instance of a program that has the following properties:

* Virtual to physical address mappings.
* A process ID.
* A group ID.
* A notification handler.
* A list of threads.
* A list of other processes watching this process.

A process is being executed by **zero or more** threads. Threads can hop into a
process at any time (see `Inter-process communication`_).
