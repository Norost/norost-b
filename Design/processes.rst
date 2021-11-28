=========
Processes
=========

A process consists of:

- one address space.
- one more more threads.
- zero or more handles to kernel objects.


Address space
~~~~~~~~~~~~~

The address space is managed by the kernel. It can map anonymous memory and
files with read, write and/or execute permissions (RWX).


Threads
~~~~~~~

A process consists of one or more threads. Each threads has a separate set of
registers. When the last/only thread exits, the process is destroyed.


Kernel objects
~~~~~~~~~~~~~~

A process has zero or more handles to kernel objects. These objects may be
shared with other processes.

Handles are represented by an integer ID.


Input/Output
~~~~~~~~~~~~

I/O is achieved through an asynchronous interface. This interface consists of
two ring buffers, one for requests by the process and one for completions by
the kernel.

To submit a request, the process adds an entry to the requests queue and
increments the head. The kernel keeps track of the tail internally.

The kernel will submit completions & errors to the completion queue. The
process has to keep track of the tail manually.


Inter-process communication
~~~~~~~~~~~~~~~~~~~~~~~~~~~

IPC is achieved through FIFO queues residing in the kernel. Read/write
operations are submitted through the I/O queue.

Shared memory is done through files.
