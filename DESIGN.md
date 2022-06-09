# Design

This document elaborates on some design decisions & the motivation behind them.


## Microkernel

The main reason is to make it trivial to replace drivers, which is useful when isolating
processes. e.g. any network operations could be proxied through a firewall, which can be
implemented as a separate process. This is completely transparent to the client.

Another is to allow high-performance applications to directly integrate drivers, e.g a
database could directly communicate with disks, reducing overhead & latency significantly.


## Asynchronous I/O

So far, asynchronous I/O using ring buffers seems to be the best-performing on modern
hardware, since it reduces privilege & context switches associated with blocking I/O.
It also scales well with increasing workloads as more work will be done between each
poll, i.e. batch size scales automatically.

Synchronous I/O is often easier to use & the right choice if the result is immediately
needed. Since this type of I/O likely doesn't need to be very performant it is
implemented on top of the existing asynchronous interface.


### Readiness vs completion

There are two common ways to handle asynchronous I/O. One is based on readiness, where
the server sends a message that an operation can be performed, and the other is based
on completion, where the server sends a message when an operation is done.

The obvious disadvantage of the former is that it requires up to two calls per operation:
one to check if an operation can be performed and one to actually perform the operation.
In contrast, the completion-based model only requires one call since an operation starts
the moment a request is sent. Since reducing latency is a great concern the I/O queue uses
the latter model.


#### Owned buffers

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
