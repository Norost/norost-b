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
