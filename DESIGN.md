# Design

This document elaborates on some design decisions & the motivation behind them.


## Microkernel

The main reason is to make it trivial to replace drivers, which is useful when isolating
processes. e.g. any network operations could be proxied through a firewall, which can be
implemented as a separate process. This is completely transparent to the client.

Another is to allow high-performance applications to directly integrate drivers, e.g a
database could directly communicate with disks, reducing overhead & latency significantly.


## I/O

There are four "levels" of API, going from ease and genericity of use to performance:

| Level         | Minimal | Application agnostic | Device agnostic |
|---------------|---------|----------------------|-----------------|
| Synchronous   | &check; | &check;              | &check;         |
| Asynchronous  | &cross; | &check;              | &check;         |
| Shared memory | &cross; | &cross;              | &check;         |
| Integrated    | &cross; | &cross;              | &cross;         |

The synchronous is the simplest and easiest to use.

The asynchronous API allows batching requests, increasing throughput compared
to the synchronous API.

The shared memory approach allows processes to define their own interchange
format and mostly bypass the kernel, potentially improving performance further
than what is possible with the default asynchronous API. It however is
application-specific, which means a program must be aware of the specialized
API to make use of it.

The integrated approach is useful if every last bit of performance needs to be
eked out. Drivers are integrated directly in the application, allowing the
compiler to perform more extensive optimizations and reducing the amount of
context and privilige switches.


### Asynchronous I/O

So far, asynchronous I/O using ring buffers seems to be the best-performing on modern
hardware, since it reduces privilege & context switches associated with blocking I/O.
It also scales well with increasing workloads as more work will be done between each
poll, i.e. batch size scales automatically.

Synchronous I/O is often easier to use & the right choice if the result is
immediately needed. While synchronous I/O can easily be implemented on top
of an asynchronous API, it is common enough a dedicated system call has been
added (`do_io`). It reduces the size of simple programs, makes the runtime
lighter and is slightly more performant.


#### Readiness vs completion

There are two common ways to handle asynchronous I/O. One is based on readiness, where
the server sends a message that an operation can be performed, and the other is based
on completion, where the server sends a message when an operation is done.

The obvious disadvantage of the former is that it requires up to two calls per operation:
one to check if an operation can be performed and one to actually perform the operation.
In contrast, the completion-based model only requires one call since an operation starts
the moment a request is sent. Since reducing latency is a great concern the I/O queue uses
the latter model.


##### Owned buffers

A subtle disadvantage of the latter model is that cancellation is not implicit: with
a readiness-based model an operation can be "cancelled" by simply ignoring the ready
message. This is not an option with completion-based models since the server may read
from / write to a buffer at any time, so cancellation has to be explicit and a buffer
must remain valid as long as an operation has not been finished or is cancelled.

One way to ensure that buffers live long enough is to let the queue manage the buffers
directly: a client moves data into a buffer and then moves this buffer to the queue. This
buffer will then remain inaccessible to the client until the queue is done with it. To
avoid redundant allocations & deallocations the queue returns the buffer to the client
when it is done with it.


## Object-oriented interface

A minimal but powerful OO interface makes it easy to isolate & scale processes.
For example, a job server can run a compile run on many different machines by
providing a custom process table, which instead of creating a new process on the
local machine will pick any machine as appropriate. It can also provide a file
table which will gather outputs from any processes to a central location.


## Multi-stage bootloader

While there are many powerful bootloaders, they also still leave much setup work
to the kernel such as setting up the page tables. To simplify things & reduce the
size of the kernel itself a separate bootloader is used. This loader passes a very
easy to parse info structure to the kernel. It handles identity-mapping, loading of
the kernel ELF binary & can pass any drivers to the kernel. It also tells the kernel
which memory regions are free & useable as regular volatile RAM.
