===========================
Inter-process communication
===========================

Goals
~~~~~

IPC communication by clients is classified in three categories:

* Low throughput:
  For applications that do not need fast I/O an extensive set of synchronous
  calls are available. These are implemented on top of high throughput
  asynchronous calls.

* High throughput, generic:
  Applications that need high I/O throughput but need to work with a variety of
  other processes can make use of an asynchronous ring buffer. 

* High throughput, specialzied:
  For systems with processes dedicated to certain tasks the devices can be
  passed directly to these processes. This will have the highest possible
  throughput of any solution.

The design described below focuses on the high throughput, generic case.


Data sharing mechanisms
~~~~~~~~~~~~~~~~~~~~~~~

Data is shared through the use of shared memory mappings. In general, there are
three types of shared memory:

* Kernel memory, where only one process shares a buffer with the kernel.
  This is used for the I/O queues.
* File memory, where any process and the kernel share a file object which may
  be cached in memory and/or be stored somewhere on a disk. This mechanism is
  backed by the VFS.
* Anonymous memory, where any process share a fixed size of memory. This is
  useful for e.g. framebuffers, which are generally static in size.


Ring buffers
~~~~~~~~~~~~

All communication rely on ring buffers to notify the kernel and processes.
There are two types of ring buffers: client buffers and server buffers.

The approach is heavily inspired by Linux' ``io_uring``, which seems to scale
the best when under high pressure.


Client buffers
''''''''''''''

The client buffer consists of two queues: the submission queue and the
completion queue.

Each entry in the submission is 64 bytes large. The first byte indicates the
opcode of the operation, the last 8 bytes are reserved for arbitrary user data
and the value of the remaining 55 bytes depend on the exact operation.

When a operation has completed, an entry is added to the completion queue. Each
entry in the completion queue has two fields: the first being the 64-bit
userdata field, the second being 64 bits of arbitrary data depending on the
performed operation.


Servers buffers
'''''''''''''''

Server buffers work much like client buffers except in the reverse way: instead
of submitting entries, entries are *received* via a submitted ring. Any entries
that have finished processing are added to a completed ring.

A submitted entry has the same structure as that of a client submission entry:
first byte for the opcode, 55 bytes for arbitrary data and the last 8 for
userdata (although in this case, it is a kernel tag). The completed entry has
the 64-bit userdata as first field and 64 bits of arbitrary data as the next.


Queue processing
''''''''''''''''

Client queues are exclusively processed by the kernel. A process can request
the kernel to explicitly scan the queue once or to do so periodically, i.e.
polling. In the latter case, a separate kernel thread is spawned.

A process with one or more server queues can request to be woken up if *any*
queue has one or more entries.
