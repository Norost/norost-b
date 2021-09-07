==============
Window manager
==============

Window managers are responsible for allocating per-client buffers and
displaying them such that the output of multiple clients can be viewed at the
same time.

A client allocates a buffer by *creating* a new object with the window manager.
If successful, the client maps the buffer associated with the object into its
own address space and begins drawing content in it. When done, it synchronizes
the buffer with the window manager.
